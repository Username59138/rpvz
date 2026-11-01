# RPvZ

A reimplementation of the Plants vs. Zombies engine in Rust and [Bevy](https://bevyengine.org).

> **Status:** early development: asset installer, format parsers and the main menu.

## Game assets

RPvZ ships no game assets. On first launch it looks for them in this order:

1. **Steam:** an installed copy of *Plants vs. Zombies GOTY* (app 3590). Native,
   Flatpak and Snap Steam on Linux are supported, plus Steam on Windows and macOS.
   Extra Steam library folders are found through `libraryfolders.vdf`.
2. **PC demo + GOTY content** (default without Steam, ~128 MB download):
   - the official free 2009 PC demo from the Internet Archive copy, pinned to a
     SHA-256 (the installer is Authenticode-signed by PopCap Games). Its
     `main.pak` is the complete 1.0 game; the demo executable is never run;
   - the free Android release (`com.ea.game.pvzfree_row` 2.9.10 from APKPure,
     pinned SHA-256), from which only content added after 1.0 is taken (Disco
     Zombie and backup dancers, achievements, holiday decorations, a few props)
     and converted to PC scale. No code from the APK is ever run.
3. **Your own files:** `--demo-file` / `--apk-file` skip the downloads.

Installed layout (`manifest.json` lists every file with its SHA-256):

```
assets/
  pc/      main.pak contents (lower-case paths)
  goty/    converted GOTY additions + resources.json (id -> file)
```

Assets are installed to the platform data directory:

| OS      | Path                                         |
|---------|----------------------------------------------|
| Linux   | `~/.local/share/rpvz/assets`                 |
| Windows | `%APPDATA%\rpvz\data\assets`                 |
| macOS   | `~/Library/Application Support/rpvz/assets`  |

Set `RPVZ_DATA_DIR` or pass `--data-dir` to use a different location.

### Asset installer CLI

```sh
cargo run -p rpvz-assets -- detect          # list the sources that were found
cargo run -p rpvz-assets -- install         # Steam, else demo + GOTY (asks first)
cargo run -p rpvz-assets -- install --yes
cargo run -p rpvz-assets -- install --demo-file setup.exe --apk-file pvz.apk
cargo run -p rpvz-assets -- status          # show what is installed
cargo run -p rpvz-assets -- verify          # re-hash installed files
```

### Format tests against real data

```sh
RPVZ_TEST_ASSETS=/path/to/android/obb RPVZ_TEST_PC_ASSETS=~/.local/share/rpvz/assets/pc \
  cargo test --release
```

## Running the game

```sh
cargo run --release -p rpvz-game              # uses the installed assets
cargo run --release -p rpvz-game -- --help
```

So far there is the main menu (buttons, hover, the click on Start Adventure).

Development options:

- `--make-devpack DIR` writes a placeholder pack (plain original shapes, no game
  art); `--devpack DIR` runs on it. Useful for CI and screenshots.
- `--boxes` draws every animation track as a labelled box instead of its image,
  to check layout against real data; `--labels` adds track names.
- `--screenshot FILE --at SECONDS [--cursor X,Y] [--click-at SECONDS]` runs a
  scripted session with a fixed time step, saves a screenshot and exits.

Linux build dependencies (Debian/Ubuntu): `libwayland-dev libxkbcommon-dev
libudev-dev libasound2-dev pkg-config`.

## Building

```sh
cargo build --release
cargo test
```

## License

RPvZ's source code is released under the [MIT License](LICENSE). The license
covers this repository only; it grants nothing for Plants vs. Zombies game
assets, which remain the property of Electronic Arts Inc.

## Disclaimer

RPvZ is an unofficial, fan-made reimplementation of the Plants vs. Zombies
engine. This project is not affiliated with, endorsed, sponsored, or approved
by Electronic Arts Inc. or PopCap Games.

"Plants vs. Zombies", "PopCap" and all related names and logos are trademarks
of Electronic Arts Inc. All other trademarks belong to their respective owners.

This repository does not contain any game assets (graphics, sounds, music).
To play, you need a copy of Plants vs. Zombies from Steam, or RPvZ will
download the free Android version on first launch and extract the assets
locally on your computer.
