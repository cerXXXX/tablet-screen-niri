# tablet-screen

Планшет как второй экран в niri: настоящий второй выход, кадры уходят в энкодер
по DMA-BUF (без копий в CPU), тач и перо работают, включается/выключается из
виджета DMS.

## Слои

```
niri ──► «второй выход» ──► screencopy DMA-BUF ──► VAAPI encode ──► adb ──► APK
  ▲            ▲                    ▲                                          │
  │            │                    │                                          │
  │   A: forced DRM connector       │        uscreen (host, форк)              │
  │   B: niri virtual output        │        + backend "wayland-dmabuf"        │
  │      (PR #3800 / форк)          │        + uscreen/src/niri.rs (адаптер)   │
  │                                 │                                          ▼
  └── niri msg output/… ◄───────────┴──────────────────────────────── тач/перо (uinput)
```

* **A — проверка (этот этап).** Свободный коннектор i915 (`HDMI-A-1`/`DP-*`)
  форсится через drm-core debugfs (`force`, `edid_override`) с синтетическим
  EDID. niri видит настоящий монитор. Правок в niri нет, ядро не пересобирается,
  всё обратимо.
* **B — боевой режим.** niri собирается из main с двумя патчами:
  `0001-…lock-surface.patch` (обои, уже есть) + PR #3800 (виртуальные выходы).
  Экран создаётся/удаляется по IPC, ядро и root не участвуют.

Захват в обоих случаях один и тот же: `zwlr_screencopy` v3 (есть уже в 26.04)
или `ext-image-copy-capture` (main, с 12.09.2026), в обоих — XRGB8888 DMA-BUF.

## Что откуда берётся

| часть | откуда | состояние |
|---|---|---|
| APK с декодом, тачем и пером | UScreen 1.2.12 | готово, не меняется |
| протокол, транспорт по adb, GUI (eframe), трей (ksni) | UScreen host | готово |
| инъекция тача/пера (uinput, ABS 0..65535) | UScreen `host/src/input.rs` | готово, нужен маппинг под niri |
| захват кадров | новый backend в `host/src/capture.rs` | **написать** |
| позиционирование/включение выхода | новый `host/src/niri.rs` | **написать** |
| виджет в панели DMS | плагин Quickshell + `uscreen` CLI | **написать** |

UScreen не умеет niri и не умеет никакой захват, кроме evdi (грабер
`evdi_helper` пишет NV12 в fifo). Меняем только источник кадров и добавляем
адаптер композитора — остальное переиспользуем.

## Каталоги

* `tools/mk-edid.py` — генератор EDID под любой режим (timings считает `cvt -r`).
* `edid/` — готовые блобы (3000x1872@60, 2400x1500@60).
* `scripts/fake-output.sh` — включить/выключить форс коннектора (root).

## Этап 1: проверить, что i915 вообще отдаёт форс-коннектор

```bash
cd ~/tablet-screen
sudo scripts/fake-output.sh HDMI-A-1 edid/tablet-3000x1872.bin on
# если niri не увидел — выйти из сессии и войти снова
# откат:
sudo scripts/fake-output.sh HDMI-A-1 - off
```

Признак успеха: `niri msg outputs` показывает второй выход с режимом
3000x1872@60, и на него можно перетащить окно
(`niri msg action move-column-to-monitor-right`).

Если i915 отказывается делать modeset без синка (в dmesg — link training /
`failed to enable`), пробуем `DP-*`, а если и там нет — сразу переходим к B.

## Замеры (этап A, 2026-11-03)

Выход `HDMI-A-1` форснут, EDID `tablet-3000x1872.bin`, niri его принял:

```
HDMI-A-1  3000x1872@59.972  logical 3000x1872 @ 1920,0  scale 1.0
eDP-1     1920x1080@60.008  logical 1920x1080 @ 0,0     scale 1.0
workspaces: у HDMI-A-1 свои 1..2, окна переносятся move-column-to-monitor-*
niri msg output HDMI-A-1 off → ядро: enabled=disabled, рендера нет
```

Потолок энкодера (ffmpeg, VAAPI на Iris Xe, заведомо лёгкий кадр):

| режим | fps | speed |
|---|---|---|
| 3000x1872@60, h264_vaapi qp18 | ~62 | 1.03x |
| 3000x1872@60, hevc_vaapi qp20 | ~60 | 1.00x |
| 2400x1500@60, h264_vaapi qp18 | ~132 | 2.21x |

Захват через `wlr-screencopy` + dmabuf (`wl-screenrec -o HDMI-A-1 -m 120`):

```
CPU wl-screenrec: 8–12 % одного ядра   (для сравнения: evdi_helper 154 %)
CPU mpv (источник движения, hwdec): 17 %
получилось 286 кадров / 9.02 с = 31.7 fps при нативном 3000x1872
статика: 0–7 fps → niri рендерит только по damage, в простое выход бесплатен
```

Вывод: узкое место больше не захват. Остались (а) энкодер — нативный 60 fps
только-только, (б) round-trip одноразового `wlr-screencopy` (запрос→кадр→копия),
который и даёт ~32 fps. Для (б) правильный инструмент — `ext-image-copy-capture`
(сессия с очередью буферов, есть в niri main с 12.09.2026), а не одноразовый
screencopy. Это же и есть аргумент переезда на main.

Ещё: wl-screenrec ругнулся, что low-power (VDEnc) энкодер недоступен без
`i915.enable_guc=2`; на обычном энкодере цифры выше.

## Замеры (этап B, niri main + PR #3800, 2026-11-03)

Выход создаётся и удаляется по IPC, без root:

```
niri msg create-virtual-output --name tablet --width 2400 --height 1500 --refresh-rate 60
niri msg output tablet position set 1920 0
niri msg remove-virtual-output tablet
```

Захват через `ext-image-copy-capture` (сессия, очередь буферов) + `linux-dmabuf`:

| режим | fps | encode p50/p95 | CPU | битрейт |
|---|---|---|---|---|
| 2400x1500@60 | 57.7 | 9.6 / 18.4 ms | ~8 % одного ядра | 20.9 Mbps |
| 3000x1872@50 | 49.8 | 16.9 / 24.5 ms | — | 35 Mbps |
| 3000x1872@60 | 53.6 (очередь) | 282 / 514 ms | — | 34 Mbps |

Только захват (без энкода): 57.6 fps, request→ready median 16.7 ms, CPU 0.4–0.6 %.
Для сравнения на evdi было 31.7 fps и 154 % CPU.

Выводы:
* native 3000x1872@60 Iris Xe не тянет — энкодер не успевает, латентность уходит
  в 300–500 мс; либо 2400x1500@60, либо native на 50 Гц;
* 35 Mbps — 13 % измеренной пропускной способности USB, битрейт не предел.

Грабли Intel + gst-va: `vapostproc` не объявляет XR24 вообще (только AR24, XB24,
AB24), поэтому захват в XR24 даёт not-negotiated, а fallback в LINEAR тоже
отвергается. Рабочая комбинация — AR24 + Intel Y-tiled (0x0100000000000002).

## Интеграция в UScreen (сделано 2026-11-03)

Форк: `~/tablet-screen/uscreen` (собран из исходников 1.2.12, `cargo test` 40/40).

Новое в нём:

| файл | что |
|---|---|
| `host/src/niri.rs` | адаптер niri: `outputs`, `enable_output` (on + scale + position по раскладке), `disable_output` (remove-virtual-output для виртуального, off для форснутого), `create_virtual`, `set_custom_mode`, `output_area` для координат тача |
| `host/src/wayland.rs` | backend захвата: ext-image-copy-capture + DMA-BUF → gst VAAPI (H.264), кормит существующий packetizer и `broadcast<VideoPacket>` |
| `host/src/ctl.rs` | control-сокет `$XDG_RUNTIME_DIR/uscreen/control.sock`: `uscreen display on\|off\|toggle\|status` |
| `host/src/input.rs` | `touch_area`: пересчёт координат тача/пера в прямоугольник планшетного выхода (у niri нет per-device маппинга) |
| `host/src/doctor.rs` | диагностика niri |

Конфиг (`~/.config/uscreen/config.toml`, правится `tools/uscreen-config.py`):

```toml
capture = "wayland"        # evdi (старый путь) | wayland (DMA-BUF, ноль копий)
output_name = "tablet"     # имя виртуального выхода
width = 2400; height = 1500; fps = 60
auto_resolution = false    # иначе планшет навяжет свои 3000x1872
bitrate = 20000
```

Зависимости backend'а: `mbuf_capture`/`mbuf_encoder` (крейты moreland как path-deps,
с локальной правкой: захват в AR24, а не XR24 — Intel `vapostproc` XR24 не умеет).

Виджет DMS: `~/.config/DankMaterialShell/plugins/tabletScreen/` (тип `widget`):
пилюля в баре (`on`/`off`/`—`), тумблер, пресеты разрешения, ссылка на окно UScreen.

## Первый живой прогон (2026-11-03, планшет подключён)

```
niri: create-virtual-output tablet 2400x1500@60 → on → scale 1.5 → position 1920,0
capture session: 2400x1500 AR24 modifier 0x0100000000000002 (Intel Y-tiled)
DMA-BUF stream: 583 frames captured, 583 packets sent, 58.2 fps     ← ноль потерь
Latency encode→display: p50 41.8 ms  p95 59.1 ms  (0 aged out)
  из них планшет decode+render: p50 34.7 ms, провод ~7.2 ms
CPU uscreen: 12.5 % одного ядра, RSS ~200 МБ
логический размер выхода: 1600x1000 при scale 1.5
```

Что поправил по ходу прогона:
* захват и энкод были последовательными — 44–47 fps; разнёс на два потока с
  очередью на 2 кадра → 58 fps;
* `codec config` рос на каждый keyframe (h264parse повторяет SPS/PPS) — теперь
  хранится одно поколение, пакет стабильные 44 байта;
* keyframe раз в секунду вместо 600 кадров: планшет не умеет просить IDR, иначе
  после переподключения картинка ждала до 10 с;
* `scale` вынесен в конфиг (`scale = 1.5`), иначе niri даёт 2.0 и логический
  десктоп всего 1200x750 при 2400x1500.

Что осталось: тач/перо (проверить попадание), латентность декодера планшета
(34 мс — это APK, не хост), установка пакетом вместо drop-in юнита.

## Тач и курсор (2026-11-03, после первого теста)

**Тач/перо.** У niri нет per-device маппинга как у KWin/Hyprland; вместо этого
выход задаётся в конфиге, иначе координаты растягиваются по активному
(сфокусированному) экрану — тап по планшету попадал на ноутбук:

```kdl
input {
    touch  { map-to-output "tablet" }
    tablet { map-to-output "tablet" }
}
```

Наш собственный пересчёт координат в `input.rs` (`touch_area`) поэтому выключен:
он складывал бы поправку дважды. Прямоугольник выхода остался только для передачи
курсора от пера обычному указателю, и он теперь обновляется каждые 10 с (я сам
меняю scale/position из конфига, и старый прямоугольник врал).

**Курсор.** `ext-image-copy-capture` не рисует курсор в кадр, если клиент не
попросил: сессия создаётся с `Options::PaintCursors`, и niri (у него есть
`session.draw_cursor()` в рендере) композитит его сам. Флаг был не выставлен —
курсор на планшете работал, но не отображался. Теперь `paint_cursor = cfg.cursor`,
по умолчанию включён; отключается `cursor = false` в конфиге.

## Откуда берётся «вязкий» курсор (2026-11-03)

Замер при 30 fps: decode+render на планшете 32.9 мс против 34.7 при 60 — то есть
это не очередь, а сам конвейер MediaCodec→SurfaceView (KEY_LOW_LATENCY уже стоит
в APK). Бюджет одного кадра от движения мыши до пикселя на планшете:

    0–16.7 мс  ожидание следующего кадра композитора (vsync, неизбежно)
    ~8 мс      энкод VAAPI
    ~7 мс      провод (adb)
    ~33 мс     декод+рендер планшета
    ─────────
    ~50–65 мс  ощущаемая задержка курсора

Уменьшить последние 33 мс можно только в APK: рисовать курсор поверх видео
силами планшета (позиция приходит по уже открытому input-сокету), тогда курсор
будет «локальным», а запаздывать будет только содержимое. Для этого нужен
Android SDK (сейчас не установлен) и пересборка APK.

## Почему коннект «сам отваливался» в простое (2026-11-03)

Симптом: планшет переподключается каждые 10–11 с, когда на экране ничего не
происходит.

Три причины сложились:
* в APK (`VideoReceiver.kt`) `soTimeout = 10000`: 10 с без единого байта —
  `Stream read timeout, reconnecting`;
* `stream.rs` после codec config ждёт IDR и все P-кадры до него выбрасывает,
  то есть до первого keyframe в сокет не уходит ничего;
* DMA-BUF-ветка считала keyframe в кадрах (`key-int-max = fps`), а флаг
  `idr_wanted`, который `stream.rs` ставит при подключении клиента, в неё не
  передавался (в EVDI-ветке он работал). В простое niri даёт 0.2–0.3 fps —
  следующий IDR наступал через минуты.

Что сделано:
* `idr_wanted` прокинут в `wayland::stream_frames`; перед кадром отправляется
  `DownstreamForceKeyUnitEvent` (`mbuf_encoder::force_keyframe`). `vah264enc`
  его чтит (`min-force-key-unit-interval` по умолчанию 0).
* keepalive: если 2 с нет новых кадров, последний кадр переотправляется с новым
  PTS. Для только что подключившегося клиента он же становится IDR, а заодно
  не срабатывает 10-секундный таймаут. Кодирование статичного кадра стоит доли
  миллисекунды.
* Проверка на живой системе: IDR после подключения приходит через ~4 с (было —
  минуты), клиент держится, переподключений каждые 10 с больше нет.

Пакет: `uscreen-dmabuf 1.2.12-4`.
