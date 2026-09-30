<div align="center">

<img src="src-tauri/icons/128x128@2x.png" width="96" alt="AgentKanban logo" />

# AgentKanban

**A desktop task board your coding agents keep up to date themselves**

Agents do the work. You sign off.

[![Release](https://img.shields.io/github/v/release/keviccz/AgentKanban?include_prereleases&color=007f89&label=release)](https://github.com/keviccz/AgentKanban/releases)
[![Downloads](https://img.shields.io/github/downloads/keviccz/AgentKanban/total?color=007f89)](https://github.com/keviccz/AgentKanban/releases)
![Windows](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?logo=windows&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-MCP%20server-B7410E?logo=rust&logoColor=white)
![MCP](https://img.shields.io/badge/MCP-stdio-6b4fbb)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

[中文](README.md) · **English**

[Download](https://github.com/keviccz/AgentKanban/releases/latest) · [Quick start](#quick-start) · [Desktop pets](#desktop-pets) · [Connect an agent](#supported-clients) · [Manual (Chinese)](docs/USAGE.md) · [FAQ](#faq)

<img src="docs/images/hero-en.png" alt="AgentKanban in light and dark themes" width="860" />

</div>

---

Run a few coding agents at once and you soon lose track of who is doing what, how far along each one is, and which one is waiting on you. AgentKanban is a small always-on-top window: through a local MCP server, **agents register their own tasks, write a plan, and report progress at each milestone**. You glance at it, add a note when one asks, and sign off when the work is done.

Everything stays on your machine. It never calls a model API, uploads nothing, and needs no account.

https://github.com/user-attachments/assets/b379ef8f-cec2-43d8-b4b8-ec94182b5271

<p align="center"><sub>AgentKanban in 34 seconds (sound on)</sub></p>

## Highlights

- **Agents track their own work**: tasks that change files appear automatically, with a goal, acceptance checks and plan steps. Plain Q&A is left out, and "don't track this" skips a task.
- **See who is moving right now**: tasks with a recent report show a live "Advancing" animation in one of seven styles, such as a pulsing dot, a heartbeat or a spinning ring. Tasks that go quiet for too long turn "Idle" and drop out of the In progress count.
- **Pixel desktop pets**: one pixel buddy per agent stands on the board and acts out its work, so you can tell at a glance who is waiting on you. [More below](#desktop-pets).
- **Interrupts only when it matters**: you get notified only when a task is blocked or needs your input. Finished tasks turn gray; review is optional, with right-click accept or archive and a 6-second undo.
- **Organized by project**: worktrees of one repository are grouped together. Pin, color-label, rename, archive or block whole projects.
- **Personal todos**: tick "Only me" to note something you are doing yourself, as To do, Doing or Done. Agents never see it.
- **Light on context**: just 3 MCP tools. A write returns a receipt of about 30 tokens, and a search returns 5 summaries by default.
- **One-click setup**: on first launch, a card on the board connects every agent client on your computer at once. Codex, Claude Code, Cursor and four more are supported, and files are backed up before the MCP config and tracking rules are written.
- **Quick to drive, nice to look at**: `Ctrl+Alt+K` to show, `Ctrl+Alt+N` to capture, ↑↓ / Enter / A / E / `/` on the board. Light, dark or system theme, 8 accent colors, Chinese or English UI, adjustable text size and opacity.
- **Data you can rely on**: SQLite with WAL lets several agents write at once, and optimistic locking stops them from overwriting each other. One-click backup, an archive center and a work summary you can copy as Markdown. In-app updates verify the signature and back up the database before installing.

<div align="center">
<img src="docs/images/activity-en.gif" alt="Heartbeat activity animation" width="360" />
</div>

## Desktop pets

<div align="center">
<img src="docs/images/pet-en.gif" alt="Pixel Agents standing on the board, one more wandering on the taskbar" width="560" />
</div>

- **Status at a glance**: while advancing, a pet pushes a box showing its step count, and the box flashes when a step is done. An amber "?" and a wave mean it is blocked or needs your input. A green check means it is done and waiting for review, and it jumps when you accept. It tosses a card up when a new task is registered, and turns gray and dozes when there is nothing to do.
- **Rides along with the board**: pets dock on the board's top edge by default and follow it when it moves or resizes. Drop one anywhere on the desktop and it stays there; drop it near the board's top edge and it docks again.
- **One click away**: hover for the status and task title; click to bring up the board on that agent's task.
- **Right-click menu**: split agents (one window each, placed separately), pin in place, always on top (on by default), gravity mode (they drop onto the taskbar and wander, flying back to the board for about 8 seconds when one needs you or finishes a task for review), and put back on the board.
- **Easy to turn off**: Settings → Desktop → Desktop pet turns them off or shows just one.

## How it works

```mermaid
flowchart LR
    A["Coding agent"] -- "MCP" --> M["agentkanban-mcp"]
    M --> D[("Local SQLite")]
    D --> B["Desktop board"]
    U(["You"]) -- "watch · sign off" --> B
```

- Agents call three stdio MCP tools: `task_list`, `task_upsert` and `task_archive`. Clients start the MCP process on demand, so tasks are recorded even while the board is closed.
- Data lives in `%LOCALAPPDATA%\AgentKanban`; the board sits in the tray and reads the latest state when opened.
- A task is identified by project folder plus `task_key`, so later sessions keep updating the same record.
- Tracking rules live in the tool descriptions and the client's global instructions. Agents update at milestones, real blockers and completion, never per command.

## Quick start

1. **Install**: download `AgentKanban_<version>_x64-setup.exe` from [Releases](https://github.com/keviccz/AgentKanban/releases/latest). It installs per user, no admin rights needed. A portable zip is also available.
2. **Connect**: on first launch, click "Connect all" on the card at the top of the board, or connect clients one by one in Settings → Agents.
3. **Restart the client**, give the agent a small task that edits a file, and watch it appear on the board.

> [!TIP]
> Switch the interface to English in Settings → Desktop → Appearance → Language (it follows Windows by default). An empty board starts with two tutorial samples that you can archive.

## Supported clients

| Client | MCP config | Global rules |
|---|---|---|
| Codex | `~/.codex/config.toml` | `~/.codex/AGENTS.md` |
| Claude Code | `~/.claude.json` | `~/.claude/CLAUDE.md` |
| DeepSeek Harness | `~/.dsh/cordis.patch.yml` | `~/.dsh/AGENTS.md` |
| Cursor | `~/.cursor/mcp.json` | — |
| OpenCode | `~/.config/opencode/opencode.json` | `~/.config/opencode/AGENTS.md` |
| Antigravity | `~/.gemini/config/mcp_config.json` | `~/.gemini/GEMINI.md` |
| Hermes | `~/.hermes/config.yaml` | — |

Any other client with stdio MCP support works too: open "Manual setup" in Settings and copy the snippet. See the [MCP guide (Chinese)](docs/MCP.md).

## A closer look

<div align="center">
<img src="docs/images/settings-en.png" alt="Settings panel" width="380" />
</div>

- **Detailed or compact list**, or collapse the board into a strip that shows only counts.
- **Project menu**: color label, rename, archive project, block project.
- **Task details**: full request, plan steps, deliverables, the agent's report history, and a note for the agent.

## FAQ

<details>
<summary><b>Does it call an LLM or upload my code?</b></summary>

No. It stores only the task text agents choose to report, in a local SQLite file, and never calls a model API. The only network access is the optional update check against GitHub Releases.
</details>

<details>
<summary><b>How much agent context does it use?</b></summary>

The three tool descriptions are about 1.5–2k tokens, and clients such as Claude Code and Codex load them on demand. A write receipt is about 30 tokens and a search returns 5 summaries. A typical task costs 3–5k tokens in total, mostly the progress text the agent writes.
</details>

<details>
<summary><b>How do I stop tracking a project, or pause for a while?</b></summary>

Tell the agent "don't track this" to skip one task. Right-click a project and choose "Block project" and every later call for it is told the project is blocked. The pause button in the footer pauses all recording while agents keep working.
</details>

<details>
<summary><b>An agent quit but its task still says "Advancing"?</b></summary>

"Advancing" only looks at the last report time (within 30 minutes by default); after that it falls back to "In progress". The board does not check whether an agent is alive, and a crashed agent is never marked done.
</details>

<details>
<summary><b>macOS or Linux?</b></summary>

Only Windows builds are published. The core and MCP server are Rust and the shell is Tauri, so a port is plausible, but the tray, shortcuts, notifications and installer are only tested on Windows.
</details>

<details>
<summary><b>Where is my data and how do I back it up?</b></summary>

In `%LOCALAPPDATA%\AgentKanban\agentkanban.sqlite3` by default. Settings → Desktop → Cleanup → "Back up now" writes a complete copy, even while agents are writing.
</details>

## Build from source

Requires Windows, Node.js, Rust (MSVC toolchain), Visual Studio C++ Build Tools and the WebView2 Runtime.

```powershell
npm ci
npm run desktop:dev      # dev mode: prepares the MCP server and opens the window
npm run desktop:build    # installer and portable zip in release/
npm run desktop:update   # build and install over the local copy, keeping data
```

`npm run dev` is a browser-only layout preview; open `http://127.0.0.1:1420/?demo&lang=en` for sample data (add `&theme=dark` for the dark theme).

```text
crates/kanban-core   data model, SQLite, project identity (Rust)
crates/kanban-mcp    stdio MCP server and fallback CLI entry (Rust)
src-tauri            desktop shell: tray, window, notifications, updates, client setup (Tauri 2)
src                  board UI (React 19 + TypeScript)
```

## Documentation

The detailed docs are in Chinese:

- [User manual](docs/USAGE.md): every feature, client setup, data and limits
- [MCP guide](docs/MCP.md): tool contract, disconnects, pause and block
- [Agent tracking rules](docs/AGENT_RULES.md)
- [Updates and releases](docs/UPDATES.md): signed builds and GitHub draft releases
- [Verification log](docs/VERIFICATION.md)
- [Release notes](docs/RELEASE_NOTES.md)

## License

The code is released under the [MIT](LICENSE) license. Bundled fonts keep their own licenses and are not covered by MIT: Manrope and Geist Mono are SIL OFL, and HarmonyOS Sans is redistributed unmodified under its own agreement (full text in `src/fonts/`).

## Acknowledgements

- [Tauri](https://tauri.app), [React](https://react.dev), [rusqlite](https://github.com/rusqlite/rusqlite)
- Fonts: [Manrope](https://github.com/sharanda/manrope) and [Geist Mono](https://github.com/vercel/geist-font) (SIL OFL); Chinese text uses [HarmonyOS Sans](https://developer.huawei.com/consumer/cn/design/resource/), bundled unmodified under its license (full text in `src/fonts/`)
