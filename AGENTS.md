# AGENTS.md

Guidance for coding agents working in this repository.

herdr-blink is a herdr plugin (`dartyuhov.blink`): a fuzzy agent switcher shown as a herdr popup, replacing the built-in `goto` picker. The product spec is `docs/SPEC.md`. The user-facing docs and keymap are in `README.md`.

## Commands

```sh
cargo build --release          # herdr runs ./target/release/herdr-blink directly from this checkout
cargo test                     # unit tests live in #[cfg(test)] modules next to the code
cargo test search::            # one module's tests; or a single test by name, e.g. cargo test terms_are_anded
cargo clippy --all-targets && cargo fmt
cargo run -- list [query]      # print picker rows in order against the live herdr session, no TUI
sh scripts/fetch-logos.sh      # download harness logos into assets/logos/ (gitignored)
```

The plugin is linked locally with `herdr plugin link "$PWD"`, and `prefix+f` is bound to `dartyuhov.blink.open`. After a code change, rebuild with `--release`; no re-link is needed. Re-link only after editing `herdr-plugin.toml`.

The `herdr` CLI prints a zoxide warning on stderr; ignore it.

## Architecture

There is one binary with subcommands, all wired in `herdr-plugin.toml`:

- `open`: the plugin action. It sends `plugin.pane.open` for the `picker` entrypoint. A `ui_busy` reply means a popup is already open, and is treated as success.
- `ui`: the popup process.
- `event`: the `[[events]]` hook for `pane.focused` and `pane.agent_status_changed`.
- `list`: a debug dump of the rows.

### Open path

It must stay fast; the spec target is under 50 ms.

- `herdr.rs` sends one `session.snapshot` over the raw Unix socket (newline-delimited JSON) rather than spawning the CLI.
- `state.rs` reads `$HERDR_PLUGIN_STATE_DIR/state.json` once, without a lock.
- `model.rs` joins agents, panes, tabs and workspaces into `Item`s and drops the focused pane.
- Housekeeping, such as pruning state for closed panes, runs only after the first frame.
- Do not add `git` subprocesses or network calls: `git.rs` reads `.git/HEAD`, `gitdir:` files and `commondir` directly.

### Data flow

- `model::build_items` produces `Item`s.
- `app::App` owns mode (Normal/Search), filter, query and selection, and recomputes `rows` on every change.
- `search::Searcher` matches each nucleo atom against every field. Atoms are AND-ed; within one atom, the best weighted field wins.
- `ui::render` draws the frame and records `LogoSlot`s.
- `graphics::Graphics` composites the logos into a single RGBA strip (`raster.rs`).

Ordering:
- With an empty query, rows sort by `model::tier_mru_cmp`: status tier, then last focus, then last status change, then snapshot order.
- With a query, rows sort by score, with `tier_mru_cmp` as the tie-breaker.

`search::Field::is_visible` decides whether a match triggers the hint line under the selected row. Agent and status count as visible because the logo and the status dot show them.

### Graphics

These constraints were verified against the herdr 0.9.1 source and are not obvious:

- A herdr popup has no pane id, so `pane.graphics.set` cannot target it. Instead, `ui` writes Kitty APC sequences to its own stdout. herdr's per-terminal ghostty-vt parses them and re-renders them, clipped to the popup.
- The cell pixel size comes from `pane.graphics.info` on the tiled pane under the popup, i.e. `focused_pane_id` in `HERDR_PLUGIN_CONTEXT_JSON`.
- The pty's TIOCGWINSZ pixel size is 0 until herdr's first resize, so `Graphics::init` is retried on `Event::Resize`.
- All visible logos are composited into one image with a fixed id. The image is deleted and re-transmitted only when the visible `LogoSlot`s change. `BLINK_NO_GRAPHICS=1` forces the fallback glyphs.
- Logo PNGs are not committed (trademarks). They come from a pinned LobeHub release via `scripts/fetch-logos.sh`, which also runs as a manifest build step and never fails the build.

### Event hooks

- Hooks run concurrently, up to 32 in flight, so `state::update` takes an exclusive `File::lock` on `state.lock` and writes through a temp file plus rename.
- `HERDR_PLUGIN_EVENT_JSON` is an envelope: `{"event":"pane_focused","data":{...}}`. The event name in that JSON is snake_case, while `HERDR_PLUGIN_EVENT` uses the dot form.
- `pane.agent_status_changed` also fires for presentation-only changes. `last_status` is stored so that only a real status change bumps `last_status_change_ms`.

### Popup behavior to keep in mind

- While the popup is open it receives every key, including herdr's prefix, so keybindings cannot reach it.
- The popup closes when the process exits.
- Jumping calls `agent.focus` with the pane id. herdr marks every pane in the target tab as seen, so they are no longer `done`.
- Kitty keyboard disambiguation is pushed on start so that `Esc` followed by a key is not merged into `Alt+key`.
