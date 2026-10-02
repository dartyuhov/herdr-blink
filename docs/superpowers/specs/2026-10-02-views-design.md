# Views: agents, workspaces, projects

This spec adds switchable views to the blink picker. You cycle them with `h`
and `l`, which `docs/SPEC.md` has reserved since v1. It covers three local
views and the model, rendering, and test changes they need. Remote machines
are out of scope and get their own spec. This spec only reserves a field for
them.

Status: approved in conversation on October 2, 2026, pending review of this
document.

## Goals

The views answer three different questions about the same session. Each view
uses attention-first, most-recently-used ordering, so the panes that need you
stay near the top wherever you look.

- **Agents** answers "which agent needs me?" It's the current picker,
  unchanged.
- **Workspaces** answers "where does everything live?" It's a tree of
  workspaces, tabs, and every pane, plain shells included.
- **Projects** answers "what's running against this repo?" It's a tree of
  repos, worktrees, and panes, because your workspaces and repos don't map one
  to one.

Non-goals for this spec:

- Remote machines (see [Remote machines](#remote-machines)).
- A config file or a configurable default view. The picker always opens on
  agents.
- Folding tree nodes. Trees are always fully expanded.

## View switching

This section describes behavior shared by all views. It's the only part of the
design that the agents view changes.

- In normal mode, `l` moves to the next view and `h` to the previous one, in
  the order agents, workspaces, projects. Both keys wrap around. In search
  mode, `h` and `l` are typed into the query as usual.
- The picker opens on agents.
- The rule line under the header becomes the view tab bar, so the list loses
  no rows. The active view is highlighted:

  ```text
   / search                   all 19  b·blocked 1  d·done 2  w·working 3  i·idle 9
  ─ agents ─┤ workspaces ├─ projects ──────────────────────────────────────────────
  ```

- The query and the status filter are shared. Switching views keeps both.
- The selection follows the node. If the selected node (a pane, or a group
  with the same identity) exists in the new view, it stays selected.
  Otherwise, the best-scoring pane row is selected when a query is active,
  and the first row is selected when it isn't.
- The footer gains `h/l view` in normal mode.
- The filter chip counts always count agent panes, excluding the focused pane,
  so they're identical in every view.

## Agents view

The agents view keeps today's behavior: agent panes only, the focused pane
hidden, and rows sorted by match score, then `tier_mru_cmp`. The only
differences are internal. Its rows come from the shared view builder, and
`Enter` produces a `Target::Agent`.

## Workspaces view

The workspaces view shows every pane in the session, grouped as workspace,
then tab, then pane. Plain shells are included.

### Structure and order

The tree has three levels, and every level sorts by attention, then by most
recent use.

- Workspaces, tabs within a workspace, and panes within a tab all sort by the
  aggregate key in [Aggregate ordering](#aggregate-ordering).
- The focused pane is shown. Its row is dimmed and its time column reads
  `here`.
- Workspace lines show the workspace label. Tab lines show the tab number,
  then the tab label when one is set (for example, `2 api`).

```text
blink                                   ●1 ⠹1   2m
  1 main                                ●1      2m
    ✻ ● Fix bug                 blink           2m
    $   zsh                     herdr-blink     here
  2 api                         ⠹1              now
    ◎ ⠹ Refactor                api             now
infra                                   ●1      1h
  ...
```

### Aggregate ordering

Group lines sort by a key computed from the panes under them. This keeps one
blocked agent from being buried in a quiet workspace.

1. The most urgent status of any agent pane under the group, in tier order
   blocked, done, working, idle, unknown. A group with no agent panes sorts
   after all groups with agents.
2. The most recent `last_focused_ms` of any pane under the group.
3. The most recent `last_status_change_ms` of any pane under the group.
4. herdr's number for workspaces and tabs, or the label for project groups.

Panes within a group sort by `tier_mru_cmp`. Plain shells sort after all agent
panes in the same group.

### Focus targets

`Enter` focuses the selected node:

| Node | Request |
|---|---|
| Workspace line | `workspace.focus` with `workspace_id` |
| Tab line | `tab.focus` with `tab_id` |
| Agent pane | `agent.focus` with `pane_id` (marks the agent seen) |
| Plain shell pane | `pane.focus` with `pane_id` |
| Focused pane | none; the popup closes |

## Projects view

The projects view shows every pane grouped by git repo, then by checkout. Panes
outside a repo are grouped by folder.

### Structure

The tree groups panes by repository identity, not by name, so two different
repos called `api` stay separate.

- **Repo lines** group panes by the repo's common git dir. The label is
  `GitInfo.repo`.
- **Worktree lines** group panes in the same repo by worktree root. The label
  is `⎇ <branch>`.
- **The middle level is skipped** when all panes in a repo share one worktree
  root. In that case, the repo line shows the branch after the repo label and
  pane rows sit directly under it.
- **Folder lines** group panes that aren't in a repo by exact cwd. The label is
  the cwd with `$HOME` replaced by `~`, drawn dimmer than repo lines. Panes
  with no cwd go under an `(unknown)` folder line.
- Repo and folder lines are top-level siblings and sort together by the
  aggregate key.

```text
herdr-blink  main                       ●1      2m
  ✻ ● Fix bug                   blink › main    2m
  $   zsh                       blink › main    here
herdr                                   ⠹1      now
  ⎇ feat/remote-api                     ⠹1      now
    ◎ ⠹ Add endpoint API        work › api      now
  ⎇ main                                        1h
    ✻ ● Review PR               work › review   1h
~/notes                                         3h
  π ● Notes                     misc › notes    3h
```

### Focus targets

Repo, worktree, and folder lines have no herdr equivalent. `Enter` on one of
them focuses the pane under it with the highest `last_focused_ms`. Ties go to
the first pane row under the line. Agent panes use `agent.focus`, and plain
shells use `pane.focus`.

## Filtering and search in trees

The status filter and the query both prune the trees. Neither changes the tree
order.

- A pane row is kept when it passes the status filter and, with a non-empty
  query, matches the query. Plain shells never pass a status filter.
- A group line is kept when at least one pane under it is kept.
- Group lines aren't matched themselves. A pane's searchable fields already
  include its workspace, tab, repo, branch, and folder, so a query like
  `infra` keeps every pane in `infra` and therefore the `infra` line.
- With a non-empty query, rows stay in aggregate order instead of score order.
  Selection follows the rule in [View switching](#view-switching).
- The hint line under the selected row works on pane rows in every view. For
  example, a branch match in the workspaces view shows `↳ branch: …`.

## Rendering

Trees reuse the agents-view row, with indentation and with the columns the tree
already conveys removed.

- Each tree level indents by two cells. Group lines have no fold markers.
- Group lines show their label, with workspace and repo lines in bold.
  Right-aligned, they show non-zero status counts as colored badges in tier
  order (for example, `●1 ⠹2 ●3`), then the most recent activity time under
  the group. Unknown-status agents aren't counted in badges.
- Pane-row columns after the logo and status dot:

  | View | Columns |
  |---|---|
  | Agents | title, `workspace › tab`, folder, time (unchanged) |
  | Workspaces | title, folder, time |
  | Projects | title, `workspace › tab`, time |

- Plain shells show a dim `$` in the logo column and no status dot. Their
  title is the stripped terminal title, then the pane label.
- Each view has its own empty message: `no other agents` in agents,
  `no panes` in the trees, and `no matches` when a query or filter removes
  every row.

### Logos at several indents

Tree pane rows sit at different depths, so their logos land in up to three
columns. Today's single-strip design assumes one column.

- `LogoSlot` gains an `x` in popup-local cells.
- `graphics` composites one strip per distinct `x`, using the existing
  `raster.rs` strip code. That's at most three images, well under herdr's limit
  of 16 layers per pane.
- Each strip has its own fixed image id. The rule that strips are deleted and
  re-sent only when the visible slots change still applies, now comparing
  slots including `x`.

## Architecture

The change moves the model from one item per agent to one item per pane and
adds a module that turns items into rows for a view.

### `model.rs`

`build_items` reads `snapshot.panes[]` instead of `snapshot.agents[]`. Each pane
record already carries `agent` and `agent_status`.

- `Item.agent` becomes `Option<String>`. Plain shells have `None`.
- `Item` gains `workspace_id`, `tab_id`, `focused`, `workspace_number`,
  `tab_number`, and `machine: Option<String>`. `machine` is always `None` in
  this spec. It's reserved for the remote-machines spec.
- `build_items` no longer drops the focused pane. Each view decides whether to
  show it.
- `tier_mru_cmp` places plain shells after all agent tiers.

### `git.rs`

`GitInfo` gains `common_dir: PathBuf` (the repo identity) and
`worktree_root: PathBuf` (the checkout identity). `discover` already resolves
both and only needs to return them.

### `view.rs` (new)

`view.rs` holds the pure functions that build rows. It has no terminal or herdr
dependencies.

- `enum View { Agents, Workspaces, Projects }` with `next` and `prev`.
- `enum Node { Pane(usize), Workspace(String), Tab(String), Repo(PathBuf),
  Worktree(PathBuf), Folder(String) }`. The `usize` is an index into the items.
  The other payloads are the group identities used for selection following.
- `struct Row { node: Node, depth: u8, matched: Option<MatchResult>, summary:
  Option<Summary> }`. `Summary` holds the badge counts and the latest activity
  time for group lines.
- `build_rows(view, items, matches, filter) -> Vec<Row>` groups, sorts, prunes,
  and flattens depth-first. `matches` is indexed by item, with `None` meaning
  "no match" when the query is non-empty.
- `target(view, items, rows, row) -> Option<Target>` resolves a row to a focus
  target, including the most-recently-used pane for project groups.

### `search.rs`

`search.rs` is unchanged, apart from handling `Item.agent` being optional.
Plain shells contribute no agent or status field.

### `app.rs`

`App` owns the current view and builds rows through `view.rs`.

- `App` gains `view: View`. `h` and `l` change it in normal mode and rebuild
  rows.
- `refilter` matches each item once, then calls `view::build_rows`. Selection
  following compares `Node` values.
- `Action::Jump` carries a `Target`: `Agent(pane_id)`, `Pane(pane_id)`,
  `Tab(tab_id)`, `Workspace(workspace_id)`, or `Close` for the focused pane.
- `count(status)` counts agent items, excluding the focused pane.

### `herdr.rs` and `main.rs`

`herdr.rs` gains `focus_pane`, `focus_tab`, and `focus_workspace`, each a single
request like `focus_agent`. `main.rs` dispatches on `Target` and records a
focus event in `state.json` for pane targets, as it does today for agents.

The `list` subcommand becomes `list [--view agents|workspaces|projects]
[query]`. For trees, it prints rows indented by depth, which lets you check a
view against the live session without the TUI.

## Performance

The open path must stay well under 50 ms. The change adds work in proportion to
plain-shell panes.

- Items grow from agent panes to all panes (19 to 25 in the current session).
- Git discovery runs for each new distinct cwd. It reads files only, with no
  subprocesses, and is cached per path.
- Row building for the trees runs only when you switch to them.

Verify with `time cargo run --release -- list` against the live session. If
plain-shell git lookups add noticeable time, defer them until the projects
view is first opened.

## Error handling

The views handle missing or stale data without hiding panes.

- If a focus request fails, for example because the pane closed after the
  popup opened, blink exits and prints the error to stderr, as `focus_agent`
  failures do today.
- A pane whose tab or workspace is missing from the snapshot goes under an
  `(unknown)` group line instead of being dropped.
- A view with no rows shows its empty message. `h` and `l` still work.

## Testing

Unit tests live next to the code, as in the rest of the repo.

- `view.rs`:
  - Workspace and project grouping, including folder groups for non-git panes.
  - Aggregate ordering: a blocked agent pulls its tab and workspace above
    others, and focus time breaks ties within a tier.
  - Pruning by query and by status filter keeps ancestors and drops empty
    groups.
  - A repo with one worktree root skips the middle level, and a repo with two
    doesn't.
  - The focused pane is absent from agents and present in both trees.
  - `target` resolves each node kind, including the most-recently-used pane for
    repo, worktree, and folder lines.
- `app.rs`:
  - `h` and `l` cycle and wrap in normal mode and type into the query in search
    mode.
  - The query and filter carry across views.
  - The selection follows a pane, and falls back to the first row or the
    best-scoring pane.
- `model.rs`: items come from `panes[]`, plain shells have `agent: None`, and
  the focused pane is kept and marked.
- `graphics.rs`: slots at several `x` values produce one strip per column.
- Manual: compare `list --view workspaces` and `list --view projects` against
  herdr's sidebar, and open the popup to check logos at each indent.

## Documentation updates

The implementation updates the existing docs in the same change.

- `docs/SPEC.md`: remove views from "Out of scope for v1", replace the
  reserved `h` / `l` row in the keymap, and link to this spec.
- `README.md`: add `h` / `l` to the keymap and describe the three views.
- `AGENTS.md`: describe per-pane items and `view.rs` in the data-flow section,
  and note multi-strip logos in the graphics section.

## Remote machines

Remote machines are a separate spec. This section records what the
investigation found, so that spec doesn't repeat it.

- In herdr 0.9.1 and on `main` as of October 2, 2026, the herdr client holds
  the live SSH connections and streamed snapshots for saved machines. The
  local server and the plugin API don't expose them.
- A plugin can list remote agents only by running
  `herdr --machine <id> api snapshot`. It opens new SSH sessions per call, so
  it must run in the background after the first frame, with a cache.
- A plugin can't switch your client's view to another machine. Only input in
  herdr's own UI can trigger that. A real remote jump needs a herdr API, such
  as a focus request that takes a machine.
- The planned approach: remote agents appear in the agents view with a machine
  label, fetched in the background and cached with stale and offline markers.
  The workspaces view gains a machine level, and remote panes in the projects
  view group by folder only. `Enter` focuses the pane on the remote machine
  until herdr adds the API.
