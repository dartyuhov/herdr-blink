# herdr-blink

A fast fuzzy agent switcher for [herdr](https://herdr.dev). It replaces the
built-in `goto` picker: agents that need attention come first, then the
rest in most-recently-used order. Rows show harness logos. Two tree views
show every pane by workspace or by git repo.

https://github.com/user-attachments/assets/7f9a0a2f-9e65-463e-8d12-ede352c8015e

## Install

Requires herdr >= 0.9.1, a Rust toolchain, and macOS or Linux. Logos need a
Kitty-graphics terminal such as Ghostty.

```sh
herdr plugin install dartyuhov/herdr-blink
```

The install builds the binary with `cargo build --release` and downloads the
logos, so the first install takes a minute. To update, run the same command
again. If you already linked a local copy, run
`herdr plugin unlink dartyuhov.blink` first, because herdr refuses to install
over a linked plugin.

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

By default, the popup opens on the agents view, which lists agent panes across
every workspace, except the pane you're in. When the query is empty, rows are
grouped by status in this order, and sorted most-recently-used within each
group:

1. `blocked`: waiting on an approval or question
2. `done`: finished, and you have not looked at it yet
3. `working`
4. `idle`
5. `unknown`

| Mode | Key | Action |
|---|---|---|
| normal | `j` / `k`, `↓` / `↑` | move selection |
| normal | `h` / `l`, `[` / `]` | previous / next view |
| normal | `/` | search |
| normal | `b` / `d` / `w` / `i` | show only blocked / done / working / idle |
| normal | same filter key again, or `a` | show all |
| normal | `Enter` | jump to the selected row |
| normal | `Esc` / `q` | close |
| search | type | filter live |
| search | `↓` / `↑`, `ctrl+n` / `ctrl+p` | move selection |
| search | `[` / `]` | previous / next view |
| search | `ctrl+w` / `ctrl+u` | delete word / clear query |
| search | `Enter` | jump to the selected row |
| search | `Esc` | back to normal mode, query kept |

### Views

`h` and `l` cycle through three views, wrapping around. `[` and `]` do the
same and also work in search mode. The query and the
status filter carry over when you switch, and the selected pane stays
selected if the new view shows it.

- **Agents:** the flat list described above.
- **Workspaces:** a tree of workspaces, tabs, and every pane, including plain
  shells (shown with `$`) and the pane you're in (shown as `here`).
- **Projects:** a tree of git repos, worktrees, and panes. The worktree level
  appears only when a repo has panes in more than one checkout. Panes outside
  a repo are grouped by folder.

In the trees, every level sorts the same way as the agents view: the group
with the most urgent agent first, then the most recently used. Group lines
show status counts, such as `●1 ⠹2`, and the latest activity under them. The
filter chips count agents only, so they're the same in every view.

Only pane rows can be selected: `j` and `k` skip the workspace, tab, repo,
worktree, and folder lines. `Enter` focuses the selected pane.

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

## Configure

blink reads `config.toml` from its herdr plugin config directory. To find the
directory, run `herdr plugin config-dir dartyuhov.blink`. It's usually
`~/.config/herdr/plugins/config/dartyuhov.blink/`. The file is optional, and
every key has a default.

```toml
# The view the popup opens on: "agents" (default), "workspaces", or "projects".
default_view = "workspaces"
```

Changes apply the next time you open the popup. If the file is invalid, the
popup opens with the defaults, and `herdr-blink list` prints the error.

## How it works

- **Open path:** one `session.snapshot` request over the herdr socket and
  reads of a small state file and the optional config file. No `git`
  subprocesses, no network.
- **State:** herdr's snapshot has no timestamps, so `[[events]]` hooks on
  `pane.focused` and `pane.agent_status_changed` run `herdr-blink event`.
  That command records `last_focused_ms` and `last_status_change_ms` per pane
  in `$HERDR_PLUGIN_STATE_DIR/state.json`. Writes hold a file lock and replace
  the file atomically. Entries for closed panes are pruned when the popup
  opens.
- **Git:** repo and branch come from walking up from the pane's cwd and
  reading `.git/HEAD` directly. Linked worktrees, which use a `gitdir:` file,
  are supported. Results are cached per path while the popup is open.
- **Logos:** visible logos are combined into one RGBA strip image per logo
  column. Tree rows sit at up to three indents, so there are at most three
  strips. Strips are re-uploaded when the visible rows change. Without
  graphics support, each harness gets a colored glyph instead.

### Logo assets

Harness logos are trademarks of their owners, so they are not committed to
this repo. `scripts/fetch-logos.sh` runs as a plugin build step and downloads
them into `assets/logos/` from the MIT-licensed
[LobeHub icon set](https://github.com/lobehub/lobe-icons), pinned to one
release. If a download fails, that harness gets its fallback glyph instead.

## Development

```sh
cargo test
cargo run -- list [--view agents|workspaces|projects] [query]
                            # print a view's rows in order, without the TUI
```

## License

blink is available under the [MIT license](LICENSE). Harness logos are not
part of this repository and remain the property of their owners.
