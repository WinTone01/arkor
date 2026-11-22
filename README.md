<p align="center">
  <img src="docs/banner.png" alt="Arkor: every game on Linux, one launcher" width="100%">
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: GPL-3.0" src="https://img.shields.io/badge/license-GPL--3.0-8b5cf6?style=flat-square"></a>
  <img alt="Linux" src="https://img.shields.io/badge/platform-Linux-8b5cf6?style=flat-square&logo=linux&logoColor=white">
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-Rust-8b5cf6?style=flat-square&logo=rust&logoColor=white">
  <img alt="Arch Linux" src="https://img.shields.io/badge/Arch-PKGBUILD-8b5cf6?style=flat-square&logo=archlinux&logoColor=white">
</p>

<p align="center">
  <b>Arkor</b> is a fast, native game launcher for Linux. It keeps Steam, Epic, GOG and
  your own Windows and Linux games in one library, runs them with the right Proton, and
  gives every game its own MangoHud, Gamescope, frame generation and upscaler settings —
  without editing a single config file.
</p>

<p align="center">
  <a href="#-install">Install</a> ·
  <a href="#-features">Features</a> ·
  <a href="#-first-steps">First steps</a> ·
  <a href="#-command-line">Command line</a> ·
  <a href="#-credits">Credits</a>
</p>

---

## ✦ Features

<table>
  <tr>
    <td width="50%" valign="top">
      <h3>📚 One library</h3>
      Finds games already installed in <b>Steam</b>, <b>Heroic</b> (Epic, GOG, sideloaded),
      <b>Lutris</b> and <b>Faugus</b>, imports games from any Wine prefix, or adds a game by
      hand. Covers from the game's store or <b>SteamGridDB</b>, categories, search, hidden games,
      play time and a gamepad-friendly <b>big-screen mode</b>.
    </td>
    <td width="50%" valign="top">
      <h3>🍷 Proton that takes care of itself</h3>
      Runs Windows games through <b>umu-launcher</b>. Proton-CachyOS, GE-Proton, DW-Proton and
      Wineland are available as <b>“Latest”</b> builds that update before each start, next to
      any fixed version you download. umu, <b>EasyAntiCheat</b> and <b>BattlEye</b> are
      installed by Arkor itself.
    </td>
  </tr>
  <tr>
    <td valign="top">
      <h3>📊 A real MangoHud editor</h3>
      A live preview with presets, then layout, metrics, look, FPS limit and VSync, logging
      and key bindings in clear tabs. One shared layout for every game, or a game's own.
    </td>
    <td valign="top">
      <h3>🚀 More frames, per game</h3>
      <b>Gamescope</b>, <b>lsfg-vk</b> (Lossless Scaling frame generation), <b>MAKO</b>,
      <b>AFMF</b>, <b>OptiScaler</b> (e.g. FSR 4 in DLSS games), <b>DLSS Enabler</b>,
      newer <b>DLSS</b> DLLs, <b>HelixSR</b> and <b>DLSS-NR</b> for AMD cards.
    </td>
  </tr>
  <tr>
    <td valign="top">
      <h3>🎨 Better pictures</h3>
      <b>vkBasalt</b>, <b>ReShade</b>, vkSumi colour grading, HDR, and one-click Proton
      and driver switches: Wayland, NTSYNC, WOW64, Reflex, Anti-Lag 2, DLSS/FSR/XeSS upgrades.
    </td>
    <td valign="top">
      <h3>🛠️ Everything a game needs</h3>
      Launch options in Steam style (<code>VAR=1 %command%</code>), commands before and after,
      a second program next to the game, winecfg and winetricks, protonfixes, clear launch logs,
      benchmarks from MangoHud logs, backups and Discord status.
    </td>
  </tr>
</table>

**And it looks the way you like:** dark or light, any accent colour, four glass styles,
animated backgrounds or your own picture, adjustable text size, English and Turkish.

## 📦 Install

### Arch Linux and Arch-based distributions (CachyOS, EndeavourOS, Manjaro…)

One command builds Arkor from source and installs it as a normal package (binary, app-menu
entry and icon):

```bash
git clone https://github.com/WinTone01/arkor.git /tmp/arkor && cd /tmp/arkor/packaging/arch && makepkg -si
```

`makepkg` asks for your password once to install the build tools (`cargo`, `git`) and the
package. Updating later is the same command. To remove Arkor: `sudo pacman -R arkor-git`.

### Any other distribution

Install Rust (from [rustup.rs](https://rustup.rs) or your package manager), then:

```bash
git clone https://github.com/WinTone01/arkor.git && cd arkor
cargo build --release -p arkor
./target/release/arkor
```

The window needs Vulkan; libraries it uses: `libxkbcommon`, `libxcb`, `wayland`,
`fontconfig`. At run time Arkor uses `curl`, `tar` and `python` (for umu-run).

### Optional tools

Arkor works with whatever is installed and tells you on its **System** page what is missing:
`mangohud`, `gamemode`, `gamescope`, `winetricks`, `steam`, `legendary` (or Heroic) for
Epic games, `vkbasalt`.

## 🎮 First steps

1. **Open Arkor.** On the first start it installs umu-launcher, the anti-cheat runtimes and,
   if you have no Proton yet, *Proton-CachyOS Latest* — you can watch it in the sidebar.
2. **Library → Add game.** Pick one of the games Arkor found on your computer, or add an
   `.exe` (or a Linux game) by hand.
3. **Play.** Open the game's page for its settings: *Overview* has the most used switches,
   and every feature says in one line what it does.
4. **Settings → Game defaults** sets the Proton for all games and what new games start with.

Steam games keep starting from Steam; Arkor shows the launch option to paste into the game's
properties (`arkor run local:<id> -- %command%`) so its settings still apply.

## ⌨️ Command line

Everything in the window is also scriptable:

| Command | What it does |
| --- | --- |
| `arkor` | Opens the window (`--tray` starts in the system tray) |
| `arkor open <page or game>` | Opens a page, e.g. `arkor open local:far-cry-6#overlay` |
| `arkor list` · `arkor detect` | Prints the library · games found in other launchers |
| `arkor add <exe> [--name N] [--linux]` | Adds a game by hand |
| `arkor launch <id>` | Starts a game with its settings |
| `arkor run <id> -- <command>` | Runs any command with a game's settings |
| `arkor tool <id> winecfg \| winetricks \| run <file>` | Prefix tools |
| `arkor proton installed \| list \| install` | Proton builds |
| `arkor backup create \| restore <file>` | Backs up or restores Arkor's data |

Run `arkor help` for the full list.

**Keyboard:** <kbd>Ctrl</kbd>+<kbd>F</kbd> search · <kbd>Ctrl</kbd>+<kbd>Enter</kbd> play ·
<kbd>Ctrl</kbd>+<kbd>Tab</kbd> next section · <kbd>Esc</kbd> back ·
<kbd>F11</kbd> big-screen mode (gamepad).

## 🗂️ Where things are

| | |
| --- | --- |
| Settings, library, game profiles | `~/.config/arkor/` |
| Launch logs | `~/.local/state/arkor/logs/` |
| umu-run, anti-cheat runtimes | `~/.local/share/arkor/` |
| Proton builds | `~/.local/share/Steam/compatibilitytools.d/` (shared with Steam and Faugus) |
| Wine prefixes of new games | `~/Games/Arkor/Prefixes/` (changeable in Settings) |

## 💜 Credits

Arkor stands on the shoulders of great Linux gaming projects:
[umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) and
[Faugus Launcher](https://github.com/Faugus/faugus-launcher) (whose umu build and anti-cheat
packages Arkor uses), [Heroic Games Launcher](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher),
[GOverlay](https://github.com/benjamimgois/goverlay), [MangoHud](https://github.com/flightlessmango/MangoHud),
[GE-Proton](https://github.com/GloriousEggroll/proton-ge-custom),
[Proton-CachyOS](https://github.com/CachyOS/proton-cachyos) and
[OptiScaler](https://github.com/optiscaler/OptiScaler). The window is built with
[gpui-kit](https://crates.io/crates/gpui-kit), icons are [Lucide](https://lucide.dev), the
font is [Inter](https://rsms.me/inter/).

## 📄 License

Arkor is free software under the [GNU General Public License v3.0 or later](LICENSE).
