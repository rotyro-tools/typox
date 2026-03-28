# Security Policy

## Supported Versions

Only the latest released version of this project is supported with security updates.

---

## Reporting a Vulnerability

If you discover a security vulnerability, **please do not open a public issue**. Instead, report it privately using [GitHub Security Advisories](https://github.com/rotyro-tools/typox/security/advisories/new).

Please include:

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested fixes

We will acknowledge your report as soon as reasonably possible.

---

## Disclosure Policy

- We aim to respond to security reports within **7 days**
- We will work on a fix and coordinate disclosure if needed
- Public disclosure should only happen **after a fix is released**

---

## Scope

Areas of particular concern for this app:

- **API key storage** — keys are stored via `tauri-plugin-store`
- **Clipboard access** — the app reads clipboard content to perform corrections
- **IPC commands** — Tauri commands are window-origin guarded
- **AI provider requests** — outbound HTTP to Anthropic, OpenAI, or local Ollama
- **AppleScript / PowerShell execution** — used for simulating keystrokes; all inputs are sanitized

---

## Thank You

We appreciate responsible disclosure and the efforts of security researchers who help keep open source projects safe.
