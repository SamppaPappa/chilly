# Chilly

**Chilly** is a free, open-source Twitch desktop client for Windows built with [Tauri 2](https://v2.tauri.app/) and Rust (with a plain HTML/CSS/JS frontend, no bundler required).

- **Website & Downloads:** https://samppalandia.wtf/chilly/
- **Privacy Policy:** [PRIVACY.md](PRIVACY.md)
- **License:** [MIT](LICENSE)

## Features

- **Built-in live notifications:** Sleek corner pop-ups with custom per-channel sounds that work even when Windows notifications or Do Not Disturb are enabled.
- **Watch streams in tabs:** Tabbed viewing (10 tabs by default, or unlimited in settings), drag-to-reorder tabs, followed channels sidebar with live counts, and pop-out windows.
- **Multistream:** Watch two streams on one tab (side by side, stacked, or picture-in-picture) with independent or dual audio, per-stream mute/close buttons, and tabbed or dual chat.
- **Chat with emotes:** Official Twitch chat enhanced with FrankerFaceZ, BetterTTV, and 7TV emotes plus Tab autocompletion and FFZ Control Center settings.
- **Automatic bonus chests & drops:** Claims channel point bonus chests on open tabs and checks your drops inventory every 15 minutes (both optional).
- **Watch streak priority (Katseluputki):** Automatically watches selected flame channels muted in the background (even minimized to tray) with drag-to-reorder priority and automatic raid guest cleanup.
- **App-level mute & volume booster:** Mute at the WebView2 level so Twitch still counts you as a viewer, or boost quiet streams up to 300% with a limiter.
- **Discover, Clips, Follow, Subscribe & Mod View:** Browse by category/language/viewer count (including fewest viewers first), create titled 15–60s clips from the top bar, follow raids automatically, and open Mod View on channels you moderate.
- **Signed self-updates:** Checks for new releases and verifies updater signatures before installing.
- **Bilingual UI:** Finnish (`fi`) and English (`en`).

## Building from source (Windows)

Prerequisites:
- [Rust](https://rustup.rs/) (stable toolchain)
- [Node.js](https://nodejs.org/) (LTS)
- Microsoft Edge WebView2 Runtime (pre-installed on Windows 11)

```bash
npm ci
npx tauri build
```

## Privacy & Security

Chilly communicates only with Twitch (`id.twitch.tv`, `api.twitch.tv`, `twitch.tv`), FrankerFaceZ CDN (for chat emotes), and `https://samppalandia.wtf/chilly/latest.json` (for update checks). No telemetry, analytics, or user credentials are ever sent to the author or any third party. See [PRIVACY.md](PRIVACY.md) for full details.

## Third-party components

- Icons: [Lucide](https://lucide.dev/) (ISC)
- Chat emotes: [FrankerFaceZ](https://www.frankerfacez.com/), BetterTTV, 7TV
- Optional ad blocking (beta, off by default): TwitchAdSolutions / vaft (MIT)

*Chilly is an independent open-source project and is not affiliated with Twitch Interactive, Inc.*
