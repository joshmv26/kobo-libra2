# Kobo Libra 2 custom system (Josh)

- `prototype/` — interactive design prototype (open `index.html`): library/file manager, clear reader, Telegram, Sudoku (1,993 puzzles), Tetris, light panel with gradual evening schedule, config-driven homescreen with background.
- Rest of the repo — fork of [Plato](https://github.com/baskerville/plato) (AGPL-3.0), the base we port the prototype onto.
- `.github/workflows/build.yml` — cloud build: produces `libra2-bundle` (a zip with `.kobo/KoboRoot.tgz`) under Actions → Artifacts.

Install: unzip the bundle into the root of the Kobo's USB drive, eject, wait for it to reboot. Stock Nickel stays as the fallback.
