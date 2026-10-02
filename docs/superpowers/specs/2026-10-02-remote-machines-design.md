# Remote machines

This spec adds agents on herdr's saved SSH machines to the blink picker. Remote
panes appear in all three views with their status, they're searchable, and
`Enter` sends you toward them as far as herdr allows. It builds on the views
spec (`2026-10-02-views-design.md`), whose "Remote machines" section records
the investigation this design starts from.

Status: approved in conversation on October 2, 2026, pending review of this
document.

## Goals

Remote panes should behave like local ones wherever herdr makes that possible.

- **Status.** A blocked or done agent on a remote machine shows up in the
  agents view next to local ones, ordered by the same attention rules.
- **Search.** Every remote pane is searchable by the same fields as a local
  pane, plus its machine.
- **Jump.** `Enter` on a remote row gets you as close to the pane as herdr's
  public interfaces allow, and says where the pane is.

Non-goals for this spec:

- Fresh-to-the-second remote status. Remote data is as of the last time the
  popup was open long enough for a fetch to finish.
- A background daemon or event-hook refresher. All remote fetching happens
  inside the popup process.
- Git information for remote panes. `git.rs` reads local files only.
- Focus history for remote panes. blink's event hooks run on the local server
  only.

## Why `Enter` can't land you on a remote pane

A herdr window is a client. It shows one machine at a time, and machine
switching lives entirely in the client: the native goto picker, the sidebar,
and notification toasts all trigger an internal `ActivateEndpoint` action that
never goes through the socket API. blink talks to the local server, which
doesn't know which machines the client is connected to.

Checked against herdr 0.9.1 (installed), 0.9.3, and `main` on October 2, 2026:

- No API method or CLI command selects a machine in the client. No focus
  method takes a machine.
- `herdr --machine <id> agent focus <pane>` focuses the pane on the remote
  server, but the client keeps showing the machine it was on.
- `notification.show` can't target a pane, so blink can't raise a toast that
  `open_notification_target` would follow to a remote pane.
- Switching to a saved machine lands on its first tab rather than the server's
  focused tab ([herdr#4375](https://github.com/herdrdev/herdr/issues/4375),
  open), so a remote pre-focus is likely invisible on 0.9.1.

Upstream status: [herdr#3820](https://github.com/herdrdev/herdr/issues/3820)
(client-level agent list and focus for plugins) was closed as not planned and
redirected to Ideas. The open discussions are
[#4396](https://github.com/herdrdev/herdr/discussions/4396) (an API to select a
saved machine, or `agent.focus` that follows across machines),
[#3749](https://github.com/herdrdev/herdr/discussions/3749)
(`herdr machine select`), and
[#4007](https://github.com/herdrdev/herdr/discussions/4007) (a cross-machine
fuzzy jump plugin, which is blink's use case). No maintainer has replied to
any of them. A draft comment for #4396 is at the end of this spec.

So `Enter` on a remote row pre-focuses the pane on its server and raises a
toast that names the machine and location. When herdr ships a client-side
machine focus, only the dispatch in [Enter on remote rows](#enter-on-remote-rows)
changes.

## Machines and the remote cache

blink learns about machines from herdr's saved catalog and keeps the last
snapshot of each one on disk.

### Machines

`herdr machine list --json` reads the saved catalog without connecting. Each
entry has `id`, `label`, `target`, `session`, `enabled`, and `selected`.
Disabled machines are ignored. blink caches the list, so the next open can show
remote rows before anything runs.

### Cache files

Each machine has one file, `$HERDR_PLUGIN_STATE_DIR/remote/<machine-id>.json`:

```json
{
  "label": "Keryx Contabo",
  "fetched_at_ms": 1790000000000,
  "last_attempt_ms": 1790000300000,
  "last_error": "Connection timed out during banner exchange",
  "snapshot": { "panes": [], "tabs": [], "workspaces": [], "...": "..." }
}
```

- `snapshot` is `result.snapshot` from `herdr --machine <id> api snapshot`.
  The parser also accepts a bare snapshot object.
- `fetched_at_ms` is the time of the last successful fetch. `snapshot` is the
  data from that fetch.
- `last_attempt_ms` and `last_error` record the most recent attempt. A
  successful attempt clears `last_error`.
- Files are written through a temp file plus rename. Each machine's file has a
  single writer, the fetch thread for that machine, and herdr allows one popup
  at a time, so no lock is needed.
- `remote/machines.json` holds the cached catalog: `id` and `label` for each
  enabled machine.

### Machine state

A machine's state is derived from its cache file and whether a fetch is
running:

| State | Condition |
|---|---|
| Refreshing | A fetch for it is running in this popup. |
| Online | The last attempt succeeded. |
| Offline | The last attempt failed. Its cached rows, if any, are shown dimmed. |
| Unknown | No cache file yet. It has no rows. |

Offline takes precedence over the age of the data: a machine whose last
attempt failed is offline even if its data is a minute old. While a fetch is
running, rows keep the look from the previous attempt (dimmed if it failed),
and only the header shows that a refresh is in progress.

## Fetching

All remote work runs in the popup process, after the first frame.

1. After the first frame, alongside `prune_state`, a thread runs
   `herdr machine list --json` and rewrites `remote/machines.json`.
2. It then starts one thread per enabled machine. Each runs
   `herdr --machine <id> api snapshot` with a 15-second cap, writes that
   machine's cache file, and sends a message on a channel.
3. `ui_loop` already wakes at least every `TICK` (80 ms). On each pass it
   drains the channel without blocking. For each message it reloads that
   machine's items, rebuilds rows through `App`, and redraws.
4. Cache files for machines that are no longer in the catalog are deleted.

If you close the popup before a fetch finishes, the thread dies with the
process. That machine's cache keeps its previous data, and its next attempt
happens on the next open. Machines that finished in time keep their new data.

The refreshing state is visible in the header (see
[Rendering](#rendering)), so you can tell when you're looking at old data.

## Model

Remote snapshots go through the same `model::build_items` as the local one.

- `build_items` gains a `machine: Option<MachineRef>` argument. For a remote
  snapshot, every item gets `machine: Some(..)`, `focused: false`, no
  `GitInfo`, and zero `PaneTimes`. The snapshot's own `focused_pane_id` is
  ignored, because `focused` means "the pane the popup was opened from".
- `load_items` reads the local snapshot as today, then each cached remote file,
  and appends the remote items. Reading the cache is file I/O only, with no
  subprocesses and no SSH.
- `Item.machine` becomes `Option<MachineRef>`, carrying the machine id, label,
  `fetched_at_ms`, and whether its last attempt failed. The
  `#[allow(dead_code)]` goes away.
- Whether a fetch is running isn't stored on items. `App` keeps the set of
  refreshing machines, which only the header reads.
- Item `order` continues across machines (local first, then machines in
  catalog order), so it stays a stable final tie-breaker.

### Identity

Remote pane, tab, and workspace ids use the same `w1:p1` form as local ones,
so they can collide. Every identity that leaves `model.rs` is qualified by the
machine.

- `view::Node` variants that hold herdr ids (`Workspace`, `Tab`) hold a
  `(Option<MachineId>, String)` pair. `Node::Pane(usize)` is already an item
  index and needs no change.
- `Node::Machine(Option<MachineId>)` is new, with `None` for Local.
- `state.json` stays keyed by local pane id. Remote panes are never written to
  it and never looked up in it.

## Views

Remote panes follow the same rules as local panes in every view, with the
machine added where a view needs it.

### Agents view

- Remote agents mix with local ones and sort by `tier_mru_cmp`, as today.
  Because they have no focus history, a remote agent sorts after the local
  agents you've focused within the same status tier.
- The location column for a remote row reads `<machine> › <workspace> › <tab>`.
  The machine segment uses its own color so it reads as a different place.

### Workspaces view

- When at least one remote machine has items, the tree gains a top level of
  machine lines: `Local`, then each machine. Machine lines carry the same
  badges and activity time as other group lines and sort by the same
  aggregate key.
- When no remote machine has items, the tree is unchanged, with no `Local`
  line.
- `Enter` on a machine line: Local closes the popup without doing anything.
  A remote machine line raises the toast without a pre-focus.

```text
Local                                   ●1 ⠹1   2m
  blink                                 ●1      2m
    1 main                              ●1      2m
      ✻ ● Fix bug               blink           2m
Keryx Contabo · offline 12m             ●1      40m
  infra                                 ●1      40m
    1 deploy                            ●1      40m
      ✻ ● Roll out              infra           40m
```

### Projects view

- Remote panes group by folder only, because blink can't read remote git
  state. The folder label is `<machine>: <path>`, so the same path on two
  machines stays two groups.
- `~` replacement uses the local `$HOME` only. A remote path that doesn't
  start with it is shown in full.

### Search

- `search::Field` gains `Machine`, matched against the machine label. Local
  items contribute no machine field.
- `Machine` counts as visible: the label is on the row in the agents and
  projects views, and on the machine line in the workspaces view.

### Filter counts

The status chip counts include remote agents, including those on offline
machines. A chip counts what you can select under it.

## Rendering

Remote state shows in two places: once in the header, and on every row of an
offline machine.

- **Header.** The tab-bar rule gains a right-aligned machine indicator when at
  least one remote machine is configured: `⠹ Keryx Contabo` while it's
  refreshing, and `Keryx Contabo offline 12m` when it's offline. With several
  machines, refreshing ones collapse to `⠹ 2 machines`, and offline ones are
  listed individually, truncated to fit.
- **Offline rows.** Every row of an offline machine is drawn in the dim style,
  including its logo, which uses the fallback glyph instead of the image so
  that dimming is visible. The workspaces view's machine line reads
  `<label> · offline <age>`.
- **Time column.** Remote panes have no focus or status-change times, so their
  time column is empty. Group lines that contain only remote panes show no
  activity time.

## Enter on remote rows

`Enter` on a remote row closes the popup immediately, raises a toast that says
where the pane is, and pre-focuses the pane on its server in the background.

### Target

`view::Target` gains one variant:

```rust
Remote {
    machine_id: String,
    label: String,
    /// `workspace › tab › title`, shown in the toast.
    location: String,
    /// What to focus on the remote server; `None` for a machine line.
    focus: Option<RemoteFocus>,
}

enum RemoteFocus { Agent(String), Tab(String), Workspace(String) }
```

`view::target` produces it for any remote row. A remote plain-shell pane maps
to `RemoteFocus::Tab` with its tab id, because the CLI's `pane focus` only
moves to a neighboring pane and there is no generic API call.

### Dispatch

`main.rs::run_ui` handles `Target::Remote` after restoring the terminal:

1. It sends `notification.show` to the local server over the socket, with the
   title `On <label>` and the body `<location>`.
2. If `focus` is set, it spawns `herdr --machine <id> agent|tab|workspace focus
   <id>` detached, in its own process group with null stdio, and doesn't wait
   for it. An offline machine would otherwise hold the popup open for about
   10 seconds.
3. It exits. Nothing is written to `state.json`.

A failed pre-focus isn't reported, because nothing waits for it. The toast
doesn't depend on it.

When herdr adds a machine-aware focus, step 2 becomes that request and the
toast is dropped. Nothing else changes.

## Performance

The open path must stay under 50 ms with remote machines configured.

- The open path reads `remote/machines.json` and one cache file per machine.
  These are plain JSON reads of the same size as a local snapshot.
- All subprocesses and SSH run after the first frame, on threads.
- Applying a fetch result rebuilds rows once per machine, which is the same
  work as a keystroke.

Verify with `time cargo run --release -- list` with a populated cache. `list`
reads the cache but never fetches.

## Error handling

Remote failures never block or crash the popup.

- A failed or timed-out fetch records `last_error` and the attempt time, and
  marks the machine offline. The previous snapshot is kept.
- A cache file that doesn't parse is treated as missing and overwritten by the
  next successful fetch.
- A remote snapshot that doesn't match the local shape produces whatever items
  parse, possibly none. The views spec's `(unknown)` group rules apply to
  remote panes whose tab or workspace is missing.
- If `herdr machine list` fails, the cached catalog is kept.

## Testing

Unit tests live next to the code. Every new code path except the subprocess
calls is pure or file-based and testable without herdr.

- `model.rs`: a remote snapshot yields items with `machine` set,
  `focused: false`, no git info, and zero times, and the remote
  `focused_pane_id` is ignored.
- `remote.rs` (new, see [Architecture](#architecture)): cache round trip in a
  temp dir; the state rules (refreshing, online, offline, unknown); parsing
  both the envelope and a bare snapshot; removing files for machines that left
  the catalog.
- `view.rs`:
  - Agents view mixes machines and sorts remote agents after focused local
    agents in the same tier.
  - Workspaces view adds machine lines only when a remote machine has items.
  - Colliding remote and local tab ids stay separate groups.
  - Projects view groups remote panes by `<machine>: <path>`.
  - `target` returns `Target::Remote` for remote panes, tabs, workspaces,
    and machine lines, and maps a remote plain shell to its tab.
- `search.rs`: a query matching the machine label matches every pane on it,
  and the hint line shows it.
- `app.rs`: applying a fetch result keeps the selection on the same node.
- Manual, once Keryx Contabo is reachable: confirm the remote snapshot has
  the local shape, time a warm and a cold fetch, check the header indicator
  and the offline dimming, and check what the client shows after `Enter` and a
  machine switch.

## Architecture

One new module owns everything about remote machines except views.

- **`remote.rs` (new).** The `Machine` and cache types, cache read and write,
  the state rules, `load_cached() -> Vec<(Machine, Value, MachineState)>` for
  the open path, and `spawn_fetches(tx)` for after the first frame. It's the
  only module that runs `herdr --machine` or `herdr machine`.
- **`model.rs`.** `build_items` takes the machine, `Item.machine` becomes
  `Option<MachineRef>`, and `load_items` (in `main.rs`) appends remote items.
- **`view.rs`.** `Node::Machine`, machine-qualified `Workspace` and `Tab`, the
  machine level in the workspaces tree, remote folder groups, and
  `Target::Remote`. It stays pure.
- **`search.rs`.** `Field::Machine`.
- **`app.rs`.** `App::replace_machine_items(machine_id, items)` swaps one
  machine's items and refilters, keeping the selection.
- **`ui.rs`.** The header indicator, the remote location prefix, and offline
  dimming.
- **`herdr.rs`.** `notify(title, body)` over the local socket.
- **`main.rs`.** The spawn after the first frame, the channel drain in
  `ui_loop`, and the `Target::Remote` dispatch.

## Documentation updates

The implementation updates the existing docs in the same change.

- `docs/SPEC.md`: describe remote rows and link to this spec.
- `README.md`: explain that remote agents come from saved machines, what
  "offline" means, and what `Enter` does on a remote row.
- `AGENTS.md`: add `remote.rs` to the architecture, note that remote fetches
  run after the first frame and never on the open path, and record the herdr
  limitation and the upstream links.
- `docs/superpowers/specs/2026-10-02-views-design.md`: point its "Remote
  machines" section to this spec.

## Appendix: draft comment for herdr#4396

For you to post, edit, or drop. blink doesn't post it.

> +1 from another plugin use case. I maintain herdr-blink, a fuzzy agent
> switcher popup (the same idea as #4007). It can list remote agents with
> `herdr --machine <id> api snapshot`, and pre-focus one with
> `herdr --machine <id> agent focus <pane>`, but the attached client stays on
> the machine it was showing, so `Enter` on a remote agent can only tell the
> user where to go.
>
> Either shape from the original post would close this for blink: a
> client-targeted `herdr machine select <id> [--pane <id>]`, or `agent.focus`
> through `--machine` also activating that machine in the foreground client.
> Fixing #4375 would help too, since a remote pre-focus is currently lost when
> switching to the machine.
>
> Checked on 0.9.1 with a macOS client: the API schema has no machine
> parameter on any focus method, and `herdr machine` has no select command.
