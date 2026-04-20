# Typox

[![Release](https://github.com/rotyro-tools/typox/actions/workflows/release.yml/badge.svg)](https://github.com/rotyro-tools/typox/actions/workflows/release.yml)
[![Coverage](https://img.shields.io/badge/coverage-100%25-brightgreen)](https://github.com/rotyro-tools/typox)
[![Commitizen friendly](https://img.shields.io/badge/commitizen-friendly-brightgreen.svg)](http://commitizen.github.io/cz-cli/)

AI-powered grammar and spelling correction app. Built with [Tauri](https://tauri.app).

Select text anywhere, trigger the global shortcut, and Typox corrects it in place — spelling, capitalization, verb forms, and obvious contextual errors — without changing your tone or style.

---

## Features

- **Works everywhere** — captures selected text from any application
- **Multiple AI backends** — Ollama (local/private), Claude (Anthropic), OpenAI
- **Floating diff widget** — shows exactly what changed before you apply
- **Editable corrections** — tweak the AI result before applying
- **Customizable shortcut** — change the global trigger from Settings
- **Auto-updates** — checks for new versions at startup
- **Launch at login** — runs silently in the menu bar

## Platforms

| Platform              | Support |
| --------------------- | ------- |
| macOS (Apple Silicon) | ✅      |
| macOS (Intel)         | ✅      |
| Windows               | ✅      |
| Linux                 | ✅      |

> **macOS:** on first launch you may see an "unidentified developer" warning — go to **System Settings → Privacy & Security** and click **Open Anyway** next to Typox. Typox also requires **Accessibility** permission to simulate keystrokes and detect the active app — go to **System Settings → Privacy & Security → Accessibility** and enable it.
>
> **Windows:** you may see a SmartScreen warning — click **More info → Run anyway**.

---

## Installation

Download the latest release for your platform from [Releases](https://github.com/rotyro-tools/typox/releases).

| Platform            | File                         |
| ------------------- | ---------------------------- |
| macOS Apple Silicon | `Typox_x.y.z_aarch64.dmg`    |
| macOS Intel         | `Typox_x.y.z_x64.dmg`        |
| Windows             | `Typox_x.y.z_x64-setup.exe`  |
| Linux               | `typox_x.y.z_amd64.AppImage` |

---

## Usage

1. Launch Typox — it appears in your menu bar
2. Select text in any application
3. Trigger the global shortcut
4. Review the diff in the widget
5. Press **Enter** to apply or **Esc** to dismiss

### Widget keyboard shortcuts

| Key     | Action              |
| ------- | ------------------- |
| `Enter` | Apply correction    |
| `Esc`   | Dismiss             |
| `E`     | Edit corrected text |
| `R`     | Retry with AI       |

---

## Configuration

Open Settings from the menu bar icon.

### AI Providers

**Ollama** (local, no API key required)

- Requires [Ollama](https://ollama.ai) running locally on port 11434
- Supports any model you have pulled

**Claude (Anthropic)**

- Requires an [Anthropic API key](https://console.anthropic.com)
- Default model: `claude-haiku-4-5-20251001`

**OpenAI**

- Requires an [OpenAI API key](https://platform.openai.com)
- Default model: `gpt-4o-mini`

### Global Shortcut

Click the shortcut field in Settings and press any key combination to change it. Default is `Ctrl+Shift+Space`.

### Other

- **Launch at login** — toggle in Settings to start Typox automatically on login
- **System prompt** — customize the AI correction instructions in Settings

---

## Development

### Prerequisites

- [Node.js](https://nodejs.org) 24+
- [Rust](https://rustup.rs) (stable)
- **Linux only:** `libxdo-dev libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`

### Setup

```bash
git clone https://github.com/rotyro-tools/typox.git
cd typox
npm ci
npm run tauri dev
```

### Scripts

| Command                 | Description                      |
| ----------------------- | -------------------------------- |
| `npm run tauri dev`     | Start dev server with hot reload |
| `npm run tauri build`   | Build production app             |
| `npm run lint`          | Run ESLint                       |
| `npm run format`        | Format with Prettier             |
| `npm run test`          | Run unit tests                   |
| `npm run test:coverage` | Run tests with coverage          |
| `npm run cm`            | Commitizen commit wizard         |
