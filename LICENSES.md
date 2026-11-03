# Лицензии в этом репозитории

Репозиторий собирает вместе несколько частей с разными лицензиями; каждая
часть сохраняет свою.

| что | лицензия | где |
|---|---|---|
| патчи к niri (`niri/0*.patch`, `niri/PKGBUILD`) | **GPL-3.0-or-later**, как и сам niri | `niri/` |
| форк UScreen (`uscreen/`, включая `host/src/{niri,wayland,ctl}.rs` и `host/evdi/uscreen_convert.c`) | **MIT** (см. `uscreen/LICENSE`) | `uscreen/` |
| крейты moreland `capture`/`encoder` | **Apache-2.0** (SPDX-заголовки сохранены, см. `uscreen/vendor/README.md`) | `uscreen/vendor/` |
| скрипты, виджет DMS, инструменты | **MIT** | `install.sh`, `scripts/`, `tools/`, `dms-plugin/` |

Патчи к niri — производная работа от GPL-кода, поэтому распространяются на тех
же условиях. Всё остальное можно брать по MIT/Apache-2.0 соответственно.

`niri/0001-lock-wallpaper.patch` — патч из проекта
[wallpaper-engine-niri](https://github.com/cerXXXX/wallpaper-engine-niri).
`niri/0002-virtual-outputs.patch` — реализация виртуальных выходов из
[PR #3800 niri-wm/niri](https://github.com/niri-wm/niri/pull/3800).
