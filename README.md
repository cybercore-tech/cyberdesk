# 🗒 cyberdesk

A local, git-backed **markdown notes portal + editor** for the
[`darknotes`](https://github.com/darkstardevx/darknotes) vault. One Rust binary,
server-rendered, no npm, no database — the filesystem is the model, git is the
history.

- Rust · `axum` 0.7 · MiniJinja · `comrak` (GitHub-flavored markdown)
- Themes from the shared Cybercore catalog (CYBERGRID), including locally
  saved custom themes and the shared active dark/light appearance
- "Terminal window" UI: monospace, dark, the cyber\* aesthetic

## What's built (Pass 1)

- **Portal** (`/`) — note count · needs-review list · recently edited · `＋ new`
  with template chips + folder picker
- **Tree sidebar** of the whole vault
- **View** (`/n/<path>`) — rendered markdown + a contents outline, `edit` / `del`
- **Editor** (`/e/<path>`) — full-height textarea in a terminal frame;
  `Ctrl/⌘+S` saves. Every save = write file + `git commit`
- **New from template** — `_templates/*.md` with `{{title}}` / `{{date}}` / `{{slug}}`
- **Delete** — removes the file + commits
- **Search** (`/search?q=`) — title / body / tag substring, with snippets

Roadmap: rename/move · CodeMirror editor + split live preview · `[[wikilinks]]` +
backlinks · tag filter · `cyberdesk build` → static site.

## Run

```bash
cargo run
# then http://127.0.0.1:8765
```

| env | default | |
|---|---|---|
| `CYBERDESK_ROOT` | `~/Vaults/darknotes` | vault working tree |
| `CYBERDESK_BIND` | `127.0.0.1:8765` | |
| `CYBERDESK_PUSH` | `0` | `git push` after each commit (else commit locally only) |
| `CYBERGRID_THEME` | unset: shared saved selection, then schema default | e.g. `dracula`, `tokyo-night`, `neon-night`; overrides saved selection |

Requires `git` on `PATH`. Cargo uses the published `cybercore` 0.8 theme
engine.

The active theme and appearance are shared through Cybercore's user config
directory (`$XDG_CONFIG_HOME/cybercore`, or `~/.config/cybercore`). The app's
theme picker includes built-in and custom catalog entries, and its sidebar
links to the standalone Theme Studio at `http://127.0.0.1:8761/`. Start it
from a Cybercore checkout with `cargo run -p cybercore-theme-studio`. See the
[Cybercore theme engine guide](https://github.com/cybercore-tech/cybercore/blob/main/docs/theme-engine.md)
for the portable JSON format and storage contract.
Open Cyberdesk pages receive server-sent theme change events and update the
shared palette and appearance without a reload. Installing or removing
catalog themes refreshes the picker automatically.

## License

MIT
