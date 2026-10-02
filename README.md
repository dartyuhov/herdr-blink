# herdr-blink

A fast fuzzy agent switcher for [herdr](https://herdr.dev). It replaces the
built-in `goto` picker: agents that need attention come first, then the
rest in most-recently-used order. Rows show harness logos.

## Install

Requires herdr >= 0.9.1, a Rust toolchain, and macOS or Linux. Logos need a
Kitty-graphics terminal such as Ghostty.

```sh
herdr plugin install dartyuhov/herdr-blink
```

For local development, build the plugin and link it:

```sh
cargo build --release
sh scripts/fetch-logos.sh
herdr plugin link "$PWD"
```

Then bind it to `prefix+f` in `~/.config/herdr/config.toml`:

```toml
[keys]
goto = ""

[[keys.command]]
key = "prefix+f"
type = "plugin_action"
command = "dartyuhov.blink.open"
description = "switch agent"
```

Run `herdr server reload-config` to apply the change.

## Use

The popup lists agent panes across every workspace, except the pane you are
in. When the query is empty, rows are grouped by status in this order, and
sorted most-recently-used within each group:

1. `blocked`: waiting on an approval or question
2. `done`: finished, and you have not looked at it yet
3. `working`
4. `idle`
5. `unknown`

| Mode | Key | Action |
|---|---|---|
| normal | `j` / `k`, `↓` / `↑` | move selection |
| normal | `/` | search |
| normal | `b` / `d` / `w` / `i` | show only blocked / done / working / idle |
| normal | same filter key again, or `a` | show all |
| normal | `Enter` | jump to agent |
| normal | `Esc` / `q` | close |
| search | type | filter live |
| search | `↓` / `↑`, `ctrl+n` / `ctrl+p` | move selection |
| search | `ctrl+w` / `ctrl+u` | delete word / clear query |
| search | `Enter` | jump to agent |
| search | `Esc` | back to normal mode, query kept |

`h` / `l` are reserved for switching views in a later version.

### Search

Search matches every field of a row:

- harness
- status word
- pane title
- tab
- workspace
- folder
- repo
- git branch or worktree
- full cwd path

Space-separated terms must all match, but each term can match a different
field, as in fzf. fzf syntax works: `'exact`, `^prefix`, `suffix$`, and
`!exclude`. A match in the agent, title, tab, or workspace ranks above a match
in the repo or branch, which ranks above a match in the full path. A match in
a field the row does not show, such as the branch or the path, appears on a
hint line under the selected row.

## How it works

- **Open path:** one `session.snapshot` request over the herdr socket and one
  read of a small state file. No `git` subprocesses, no network.
- **State:** herdr's snapshot has no timestamps, so `[[events]]` hooks on
  `pane.focused` and `pane.agent_status_changed` run `herdr-blink event`.
  That command records `last_focused_ms` and `last_status_change_ms` per pane
  in `$HERDR_PLUGIN_STATE_DIR/state.json`. Writes hold a file lock and replace
  the file atomically. Entries for closed panes are pruned when the popup
  opens.
- **Git:** repo and branch come from walking up from the pane's cwd and
  reading `.git/HEAD` directly. Linked worktrees, which use a `gitdir:` file,
  are supported. Results are cached per path while the popup is open.
- **Logos:** all visible logos are combined into one RGBA strip image placed
  over the logo column, and the strip is re-uploaded when the visible rows
  change. Without graphics support, each harness gets a colored glyph
  instead.

### Logo assets

Harness logos are trademarks of their owners, so they are not committed to
this repo. `scripts/fetch-logos.sh` runs as a plugin build step and downloads
them into `assets/logos/` from the MIT-licensed
[LobeHub icon set](https://github.com/lobehub/lobe-icons), pinned to one
release. If a download fails, that harness gets its fallback glyph instead.

## Development

```sh
cargo test
cargo run -- list [query]   # print the picker rows in order, without the TUI
```
