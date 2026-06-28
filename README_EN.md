# CodexElves

<p align="center">
  <img src="docs/images/icon.png" alt="CodexElves icon" width="160">
</p>

<p align="center">
  <a href="README.md">中文</a> | English
</p>

<p align="center">
  <img alt="Release" src="https://img.shields.io/github/v/release/junxin367/CodexElves">
  <img alt="Stars" src="https://img.shields.io/github/stars/junxin367/CodexElves">
  <img alt="License" src="https://img.shields.io/github/license/junxin367/CodexElves">
  <img alt="Rust" src="https://img.shields.io/badge/rust-1.85%2B-orange">
  <img alt="Tauri" src="https://img.shields.io/badge/tauri-2.x-24C8DB">
</p>

CodexElves is an external enhancement launcher and manager for the Codex App. It does not modify the original Codex installation. Instead, it starts Codex externally and injects enhancements through the Chromium DevTools Protocol.

## Quick Start

Download the latest installer from [GitHub Releases](https://github.com/junxin367/CodexElves/releases):

- Windows: `CodexElves-*-windows-x64-setup.exe`
- macOS Intel: `CodexElves-*-macos-x64.dmg`
- macOS Apple Silicon: `CodexElves-*-macos-arm64.dmg`

After installation, two entry points are available:

- `CodexElves`: a silent launcher. It does not show the manager UI and only starts Codex with CodexElves injection.
- `CodexElves Manager`: a Tauri control panel for launch, diagnostics, repair, updates, relay injection, enhancements, and user scripts.

The Windows installer creates desktop and Start Menu shortcuts. The macOS DMG installs `/Applications/CodexElves.app` and `/Applications/CodexElves 管理工具.app`.

## Highlights

- Rust backend and silent launcher with no extra runtime requirement.
- Tauri + React manager with dark/light theme support.
- External CDP injection. No `app.asar` patching and no DLL writes into the Codex installation.
- Relay injection mode with multiple relay profiles, `CodexElves` provider configuration, and a one-click switch back to official ChatGPT login mode.
- Traditional enhancement mode with plugin entry unlock, forced plugin install, session delete, Markdown export, project move, and more.
- Independent user script management with startup injection.
- Provider Sync to keep historical sessions visible after switching providers.
- Upstream worktree creation: create new worktrees from `upstream/<base-branch>` after fetching the remote branch, reducing conflicts caused by stale local HEAD state.
- GitHub Release updates. Both the manager and silent launcher can detect available updates.
- Windows single instance, no console window, administrator manifest, and system Desktop path detection.
- Separate macOS x64 and arm64 DMGs. The silent launcher hides its Dock icon.

## Screenshots

## Full Feature Set
![Full feature set](image.png)

### Local Proxy

- The local proxy improves protocol translation for non-GPT models, so Codex built-in tools such as `web_search` and `tool_search` can be used.
- Custom models and context window sizes can be configured manually.

![Local proxy](image-4.png)

### Local Proxy And Request Logs
![Local proxy and request logs](image-1.png)

### Batch Session Deletion
![Batch session deletion](image-2.png)

### Codex Radar
![Codex Radar](image-3.png)

## Relay Injection

Relay injection is for users who are already logged in with an official ChatGPT account in Codex/ChatGPT and want model requests to go through a custom compatible API.

The boundary of this hybrid mode is:

- The official ChatGPT/Codex login state still owns Codex App account features and the plugin entry.
- The relay profile only controls the Base URL, key, and model names used for model requests.
- The compatible API provider is not tied to any specific vendor; it only needs to match the selected upstream protocol and Codex configuration.
- Clearing API mode should return Codex to the official login mode so the official account and plugins keep working.

Before applying relay injection, run a minimal preflight:

1. Make sure Codex has detected the ChatGPT login state and the plugin entry is available.
2. Confirm the custom Base URL is reachable and supports the selected upstream protocol, such as a Responses-compatible endpoint.
3. Test the target key with the smallest useful auth probe, such as a model-list request or a short message request.
4. Only record whether the key exists and whether auth passed. Do not paste real keys into logs, screenshots, or issues.
5. Make sure `~/.codex/config.toml` has a backup so clearing API mode can safely roll back.

In the manager's Relay Injection page:

1. Make sure ChatGPT login status is detected.
2. Add one or more relay profiles with Base URL and Key.
3. Select the active profile and apply relay injection.
4. Launch `CodexElves`.

CodexElves writes configuration similar to this into `~/.codex/config.toml`:

```toml
model_provider = "CodexElves"

[model_providers.CodexElves]
name = "CodexElves"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://example.com/v1"
experimental_bearer_token = "sk-..."
```

To return to the official login mode, use the clear API mode button in the Relay Injection page. This removes `OPENAI_API_KEY` related configuration and switches Codex back to official ChatGPT authentication.

## Enhancements

Enhancements are controlled in the manager. Enhancement injection is enabled by default. When disabled, CodexElves will not inject its menu or scripts.

When relay injection mode is active, plugin entry unlock and forced plugin install are unnecessary, and the UI will say so. Other enhancements, including session delete, export, move, and user scripts, can still be used.

## Updates and Packages

CodexElves publishes installers through GitHub Releases. Windows builds an NSIS installer, while macOS builds separate Intel x64 and Apple Silicon arm64 DMGs.

The manager's About page can check and start updates. When the silent launcher finds a new version, it opens the manager directly on the update prompt.

## Data Locations

- Codex config: `~/.codex/config.toml`
- Codex auth state: `~/.codex/auth.json`
- Codex local database: prefers `~/.codex/sqlite/*.db`, falls back to legacy `~/.codex/state_5.sqlite`
- CodexElves state and logs: `~/.codex-session-delete/`
- Provider Sync backups: `~/.codex/backups_state/provider-sync`

## FAQ

### The CodexElves menu does not appear

Make sure Codex was launched from the `CodexElves` entry instead of the original Codex entry. You can also inspect the Diagnostics and Logs pages in the manager.

### The plugin says the backend is disconnected

First test the helper endpoint:

```powershell
Invoke-RestMethod -Method Post -Uri http://127.0.0.1:45221/backend/status -Body "{}" -ContentType "application/json"
```

If the endpoint works but the plugin still times out, it is usually a Codex page CDP bridge or script cache issue. Restart CodexElves, or check manager logs for `renderer.script_loaded`, `bridge.request`, and `bridge.response`.

### How is Upstream worktree different from Codex native creation?

CodexElves updates the remote branch first, then creates the worktree as if you ran:

```bash
git worktree add -b <new-branch> <worktree-path> upstream/<base-branch>
```

The new worktree starts from the fresh remote tracking branch instead of the local HEAD used by the current session. If CodexElves cannot safely recognize the current Codex version's native worktree form, use the CodexElves menu entry and enter the repository path, branch name, worktree path, remote, and base branch manually.

### macOS says the app cannot be opened or is damaged

Unsigned and unnotarized builds may be blocked by Gatekeeper. Allow the app in System Settings -> Privacy & Security. For formal distribution, configure Apple Developer ID signing and notarization.

### Does it support Intel Macs?

Yes. Releases provide both `macos-x64.dmg` and `macos-arm64.dmg`. Intel Macs should use the x64 package, while Apple Silicon Macs should use the arm64 package.

## Development

```bash
# Frontend checks
cd apps/codex-elves-manager
npm install
npm run check
npm run vite:build

# Rust checks
cd ../..
cargo fmt --check
cargo test
cargo build --release
```

Project structure:

```text
apps/
  codex-elves-launcher/          Silent launcher
  codex-elves-manager/           Tauri manager
assets/inject/
  renderer-inject.js            Lightweight bootstrap injected into Codex
  renderer-features.js          Full enhancement runtime installed on demand
crates/
  codex-elves-core/              Launch, injection, config, update, install, bridge
  codex-elves-data/              Session data, export, Provider Sync
scripts/installer/
  windows/CodexElves.nsi     Windows NSIS installer
  macos/package-dmg.sh          macOS DMG packager
```

## Notes

CodexElves is an external enhancement tool and does not modify original Codex App files. If a future Codex App update changes page structure, the injection script may need updates.
