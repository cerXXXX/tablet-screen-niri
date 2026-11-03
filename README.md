# tablet-screen

Планшет Android как **настоящий второй экран** в niri: отдельный выход, окна
перетаскиваются, тач и перо работают, картинка идёт по USB.

```
niri ── virtual output "tablet" ──┐
                                  ├─ ext-image-copy-capture (DMA-BUF, без копий в CPU)
                                  ▼
                        VAAPI H.264 на iGPU ── adb/USB ──▶ APK UScreen (декод + тач/перо)
```

Замеры на этом ноутбуке (Iris Xe, планшет 3000×1872):

| | было (EVDI) | стало (DMA-BUF) |
|---|---|---|
| fps | 10–15 (рывками) | **59**, ноль потерь пакетов |
| CPU демона | 154 % одного ядра | **12.5 %** |
| задержка encode→экран | 22–50 мс при 10–15 fps | **p50 42 мс** при 60 fps |
| в простое | рендер и кодирование всегда | **0.6 fps / 0.5 % CPU** (выхода нет) |

Актуальные подробности замеров и история решений — в [`docs/NOTES.md`](docs/NOTES.md).

## Что ставится

| пакет / файл | что это |
|---|---|
| `niri-lock-wallpaper-tablet` | niri из main **с двумя патчами**: обои под прозрачным локскрином + виртуальные выходы (`niri msg create-virtual-output`) |
| `uscreen-dmabuf` | форк UScreen: захват DMA-BUF композитора и кодирование на iGPU; **без окна и без трея** — единственный интерфейс это виджет DMS; старый путь через EVDI остался как `capture = "evdi"` |
| `dms-plugin/tabletScreen` | виджет DMS: пилюля в баре, тумблер, пресеты разрешения |
| `input { touch/tablet { map-to-output } }` в `config.kdl` | без этого тач и перо уезжают на сфокусированный экран |

**Про имя пакета niri.** `niri-lock-wallpaper-tablet` — это тот же патч обоев,
что был в `niri-lock-wallpaper`, плюс виртуальные выходы. Пакет объявляет
`replaces=(niri-lock-wallpaper)` и `provides=("niri-lock-wallpaper=$pkgver")`,
так что pacman пишет «replaces», а не «удаление», и `pacman -S
niri-lock-wallpaper` не откатит тебя на сборку без виртуальных выходов.
`niri --version` печатает `(…, lock-wallpaper+virtual-output)`.

## Установка

```bash
./install.sh              # всё: оба пакета, плагин, маппинг ввода, перезапуск
./install.sh --no-niri    # только UScreen + плагин (например, после git pull)
```

Скрипт сам зовёт `makepkg -si` (спросит пароль), копирует плагин в
`~/.config/DankMaterialShell/plugins/tabletScreen`, добавляет блок маппинга в
`~/.config/niri/config.kdl` (с бэкапом) и перезапускает `uscreen.service`.

После установки/обновления niri нужно **выйти и войти** — иначе работает
старый композитор.

## Структура

```
install.sh                 установка всего
niri/PKGBUILD              niri: git main@a8562a1 + три патча
niri/0001-lock-wallpaper.patch      обои под локскрином (патч из wallpaper-engine-niri)
niri/0002-virtual-outputs.patch     PR #3800 — виртуальные выходы
niri/0003-version-string.patch      чтобы `niri --version` называл оба патча
uscreen/                   форк UScreen 1.2.12 + host/src/{niri,wayland,ctl}.rs
uscreen/vendor/            крейты moreland capture/encoder (Apache-2.0, см. vendor/README.md)
uscreen/PKGBUILD           сборка из этого же дерева
dms-plugin/tabletScreen/   виджет DMS (plugin.json + два QML)
tools/                     mk-edid.py (генератор EDID), uscreen-config.py (правка конфига)
scripts/                   проверки: форс коннектора, замер виртуального выхода
edid/                      готовые EDID-блобы (для пути через форс коннектора)
docs/NOTES.md              журнал замеров и решений
```

## Управление

```bash
uscreen display on|off|toggle|status   # экран: выход создаётся и удаляется
uscreen config                        # все настройки, key = value
uscreen config set bitrate 25000      # применяется на ходу, без рестарта
uscreen config mode 2400x1500@60      # размер и Гц выхода (+ в профилях планшетов)
uscreen doctor                        # диагностика: niri, VAAPI, adb, ввод
```

Виджет в баре DMS — **единственный интерфейс**: тумблер, текущий режим и все
настройки (backend, сторона, scale, курсор, режим, битрейт, качество). Он ходит
через `uscreen config`, поэтому изменения применяются к работающему пайплайну
сразу — без `systemctl restart` и без чёрного кадра. Окна настроек и иконки в
трее в этом форке нет; демон — обычный user-сервис, который видно только в
`journalctl --user -u uscreen`.

Ключи конфига `~/.config/uscreen/config.toml`, которые важны здесь:

```toml
capture = "wayland"     # wayland (DMA-BUF) | evdi (старый путь)
output_name = "tablet"  # имя виртуального выхода; оно же в map-to-output
width = 2400; height = 1500; fps = 60   # 3000x1872@60 Iris Xe не тянет
scale = 1.5             # логический размер 1600x1000
cursor = true           # рисовать курсор в кадре (иначе он невидим на планшете)
bitrate = 20000
```

## Почему такие числа

* **2400×1500@60, а не нативные 3000×1872@60**: энкодер Iris Xe на нативном
  разрешении выдаёт ~54 fps, очередь растёт и задержка уезжает в 300–500 мс.
  На 2400×1500 — стабильные 59 fps с запасом. Нативно — только на 50 Гц.
* **`capture = "wayland"`**: EVDI отдаёт кадр только через системную память,
  копия 22.5 МБ на кадр на CPU. DMA-BUF — это тот же буфер, в который рисовал
  композитор.
* **AR24, а не XR24**: `vapostproc` из gst-va не объявляет XR24 вообще; рабочий
  вариант — AR24 + Intel Y-tiled (0x0100000000000002).
* **`map-to-output` в конфиге niri**: у niri нет per-device маппинга, он
  растягивает абсолютное устройство на *сфокусированный* выход.

## Отладка

```bash
journalctl --user -u uscreen -f        # DMA-BUF stream: … fps, Latency p50/p95
niri msg outputs                       # есть ли выход tablet и в каком режиме
uscreen doctor                         # EVDI/uinput/VAAPI/niri/adb
./scripts/check-virtual-output.sh      # создать выход и замерить захват пробником
```

Симптом → причина:

* на планшете нет курсора, но клики работают → `cursor = false` (или композитор
  не поддерживает `PaintCursors`);
* тап попадает на ноутбук → нет `input { touch { map-to-output "tablet" } }`;
* поток 0.5–3 fps на пустом экране → это норма: niri рендерит только по damage;
* `not-negotiated` из GStreamer → захват попросил XR24 вместо AR24.

## Обновление niri

`niri/PKGBUILD`: поставить новый `_commit` из main, `updpkgsums`, поднять
`pkgrel` (или `pkgver`), запустить `./install.sh --rebuild-niri`.

Патчи независимы: 0001 — обои, 0002 — PR #3800, 0003 — строка версии. Если
PR #3800 вольют в апстрим, 0002 (и, возможно, 0003) просто выпадут.
