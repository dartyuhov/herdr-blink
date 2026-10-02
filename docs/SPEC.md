# herdr-blink — spec

A blazing-fast fuzzy agent switcher for herdr. Replaces the built-in `goto`
picker on `prefix+f`.

- Plugin id: `dartyuhov.blink` (action `dartyuhov.blink.open`)
- Repo: `dartyuhov/herdr-blink`
- Binary: `herdr-blink`
- Runtime: Rust (`ratatui` + `crossterm`, `nucleo` matcher)
- Target: herdr >= 0.9.1, macOS + Linux, Ghostty (Kitty graphics)

## Goals

1. **Fast.** Popup visible with populated rows in well under 50 ms after the
   keypress. Open path = one `session.snapshot` request + one small state file
   read. No `git` subprocesses, no network.
2. **Better ordering** than built-in `goto`: attention first, then MRU.
3. **Better look & feel**: real harness logos, dense informative rows,
   catppuccin popup style matching the user's other popups (`#1e2030` bg).

## Trigger & placement

- `config.toml`: set `goto = ""`, bind `prefix+f` →
  `type = "plugin_action"`, `command = "dartyuhov.blink.open"`.
- Renders as a centered `popup` plugin pane; closes after a jump.
- Re-invoking `open` while the popup is open focuses the existing popup
  (no duplicates).

## Items (v1)

- Agent panes across **all workspaces** in the session.
- The currently focused pane is **hidden**.
- Configurable scope (plain panes / tabs / workspaces) and extra **views** are
  deferred; `h` / `l` are reserved for cycling views.

## Ordering (empty query)

Tiers, top → bottom, MRU within each tier:

1. `blocked` — waiting on an approval/question UI
2. `done` — finished, not yet seen (server-side seen state)
3. `working`
4. `idle`
5. `unknown`

There is no error tier: herdr has no error status and exited agents leave the
agent list.

With a non-empty query, rows are ordered by match score (tier/MRU as
tie-breakers).

## Row content

`[logo] [status] pane title   workspace › tab   cwd/repo hint   2m`

- **Logo**: real harness logo via herdr's native `pane.graphics.set` API
  (direct-kitty transport). Day-one logos: Claude Code, Codex, OpenCode, Pi,
  GitHub Copilot. Others get a colored fallback glyph. Fallback glyphs are also
  used when graphics are unavailable.
- **Status**: colored dot; spinner for `working`.
- **Pane title**: `terminal_title_stripped` (or pane label).
- **Location**: `workspace › tab` labels.
- **Last activity**: relative time since last status change / focus.
- Matched characters are highlighted. If the best match is in a field not shown
  in the row (full path, branch), a hint line is shown under the selected row.

## Search

Fuzzy search matches across **every field** of a row:

- harness / agent kind (`claude`, `codex`, `opencode`, `pi`, `copilot`)
- status word (`blocked`, `done`, `working`, `idle`, `unknown`)
- pane title
- tab label
- workspace label
- folder: full cwd path and its basename
- repo name and git branch / worktree
- project (repo root basename)

Rules:

- Space-separated terms are AND-ed; each term may match any field (fzf-style).
- Fields are weighted: agent / pane title / tab / workspace above repo/branch
  above full path.
- Search applies inside the active quick filter.

## Modes & keys

Opens in **normal mode**.

| Mode | Key | Action |
|---|---|---|
| normal | `j` / `k`, `↓` / `↑` | move selection |
| normal | `h` / `l` | cycle views (reserved; no-op in v1) |
| normal | `/` | enter search mode |
| normal | `b` / `d` / `w` / `i` | exclusive filter: blocked / done / working / idle |
| normal | same filter key again, or `a` | clear filter (All) |
| normal | `Enter` | jump to selected agent |
| normal | `Esc` / `q` | close popup |
| search | typing | update query live |
| search | `↓` / `↑`, `ctrl+n` / `ctrl+p` | move selection |
| search | `Enter` | jump to selected agent |
| search | `Esc` | back to normal mode, query kept |

Jumping uses `agent.focus` (marks the agent seen).

## State tracking

The snapshot has no timestamps or focus history, so blink keeps its own:

- `[[events]]` hooks on `pane.focused` and `pane.agent_status_changed` run
  `herdr-blink event`, which updates `$HERDR_PLUGIN_STATE_DIR/state.json`
  (`pane_id → { last_focused_ms, last_status_change_ms }`), written atomically.
- Entries for closed panes are pruned lazily.
- Repo / branch are derived by walking up from the pane cwd and reading
  `.git/HEAD` (and `gitdir:` files for worktrees) directly; cached per path for
  the lifetime of the popup.

## Graphics notes (herdr 0.9.1)

- `pane.graphics.info` reports `file_frame_transport: "direct-kitty"`, cell size
  (e.g. 23×40 px), and `max_layers_per_pane: 16`.
- Because of the 16-layer cap, all visible logos are composited into **one**
  RGBA strip image placed over the logo column; it is re-uploaded on
  scroll/filter changes.
- Logo assets: decide between committing PNGs vs fetching at build time
  (trademark hygiene for a public repo).

## Open risks / spikes

1. `pane.graphics.set` on a plugin **popup** pane — verify first.
2. End-to-end open latency (herdr popup spawn + snapshot + first frame).
3. Event hook process spawn cost on busy sessions (status changes are frequent).

## Out of scope for v1

- Views (cycled with `h` / `l`), configurable item scope, query prefixes.
- Non-agent jump targets (plain shells, tabs, workspaces).
- Close/kill from the picker, quick-jump digits.
- Remote machines (`--machine`).
- Prebuilt release binaries (v1 builds with `cargo build --release`).
