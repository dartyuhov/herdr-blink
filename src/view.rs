//! Rows for each view: the flat agents list and the workspace and project
//! trees. Pure functions over items, with no terminal or herdr access.
//!
//! Trees are built from every pane, sorted, then pruned while flattening, so
//! the status filter and the query never change the tree order.

use std::{
    cmp::Reverse,
    collections::HashMap,
    hash::Hash,
    path::{Path, PathBuf},
};

use crate::{
    model::{Item, Status, UNKNOWN, tier_mru_cmp},
    search::MatchResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Agents,
    Workspaces,
    Projects,
}

impl View {
    pub const ALL: [View; 3] = [View::Agents, View::Workspaces, View::Projects];

    pub fn next(self) -> View {
        Self::ALL[(self as usize + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> View {
        Self::ALL[(self as usize + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn name(self) -> &'static str {
        match self {
            View::Agents => "agents",
            View::Workspaces => "workspaces",
            View::Projects => "projects",
        }
    }

    pub fn parse(name: &str) -> Option<View> {
        Self::ALL.into_iter().find(|v| v.name() == name)
    }

    /// Shown when the view has no rows before any query or filter.
    pub fn empty_message(self) -> &'static str {
        match self {
            View::Agents => "no other agents",
            View::Workspaces | View::Projects => "no panes",
        }
    }
}

/// A row's identity, stable across views so the selection can follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// Index into the items.
    Pane(usize),
    Workspace(String),
    Tab(String),
    /// Repo identity: the common git dir.
    Repo(PathBuf),
    /// Checkout identity: the worktree root.
    Worktree(PathBuf),
    /// Exact cwd of panes outside a repo; empty for an unknown cwd.
    Folder(String),
}

/// Statuses counted in group badges, in tier order. Unknown is not counted.
pub const BADGES: [Status; 4] = [Status::Blocked, Status::Done, Status::Working, Status::Idle];

/// What a group line shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub label: String,
    /// Text after the label: the branch of a repo with a single checkout.
    pub detail: Option<String>,
    /// Agent panes under the group per status, in `BADGES` order.
    pub counts: [usize; 4],
    pub last_activity_ms: u64,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub node: Node,
    pub depth: u8,
    pub matched: Option<MatchResult>,
    /// Set on group lines only.
    pub summary: Option<Summary>,
}

impl Row {
    pub fn item(&self) -> Option<usize> {
        match self.node {
            Node::Pane(i) => Some(i),
            _ => None,
        }
    }
}

/// What `Enter` focuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `agent.focus`, which also marks the agent seen.
    Agent(String),
    /// `pane.focus`, for plain shells.
    Pane(String),
    Tab(String),
    Workspace(String),
    /// The pane the popup was opened from: just close.
    Close,
}

/// Builds the rows of `view`. `matches` is `None` for an empty query;
/// otherwise it is indexed by item, with `None` meaning "no match".
pub fn build_rows(
    view: View,
    items: &[Item],
    matches: Option<&[Option<MatchResult>]>,
    filter: Option<Status>,
) -> Vec<Row> {
    let keep = |i: usize| {
        let item = &items[i];
        filter.is_none_or(|f| item.is_agent() && item.status == f)
            && matches.is_none_or(|m| m[i].is_some())
    };
    let matched = |i: usize| matches.and_then(|m| m[i].clone());
    let mut rows = Vec::new();
    match view {
        View::Agents => {
            rows.extend(
                (0..items.len())
                    .filter(|&i| items[i].is_agent() && !items[i].focused && keep(i))
                    .map(|i| pane_row(i, 0, matched(i))),
            );
            rows.sort_by(|a, b| {
                let score = |r: &Row| r.matched.as_ref().map_or(0, |m| m.score);
                let item = |r: &Row| &items[r.item().unwrap_or_default()];
                score(b)
                    .cmp(&score(a))
                    .then_with(|| tier_mru_cmp(item(a), item(b)))
            });
        }
        View::Workspaces => flatten(workspaces_tree(items), 0, &keep, &matched, &mut rows),
        View::Projects => {
            let home = std::env::var("HOME").unwrap_or_default();
            flatten(projects_tree(items, &home), 0, &keep, &matched, &mut rows);
        }
    }
    rows
}

fn pane_row(item: usize, depth: u8, matched: Option<MatchResult>) -> Row {
    Row {
        node: Node::Pane(item),
        depth,
        matched,
        summary: None,
    }
}

/// Resolves the row at `row` to a focus target.
pub fn target(items: &[Item], rows: &[Row], row: usize) -> Option<Target> {
    let has = |f: &dyn Fn(&Item) -> bool| items.iter().any(f);
    match &rows.get(row)?.node {
        Node::Pane(i) => Some(pane_target(&items[*i])),
        Node::Workspace(id) if has(&|i| i.workspace_id == *id && !i.workspace.is_empty()) => {
            Some(Target::Workspace(id.clone()))
        }
        Node::Tab(id) if has(&|i| i.tab_id == *id && !i.tab.is_empty()) => {
            Some(Target::Tab(id.clone()))
        }
        // Lines with no herdr equivalent (and unknown workspaces or tabs)
        // focus their most recently used pane.
        _ => mru_pane(items, rows, row).map(|i| pane_target(&items[i])),
    }
}

fn pane_target(item: &Item) -> Target {
    if item.focused {
        Target::Close
    } else if item.is_agent() {
        Target::Agent(item.pane_id.clone())
    } else {
        Target::Pane(item.pane_id.clone())
    }
}

/// The pane row under the group line at `row` with the highest
/// `last_focused_ms`; ties go to the first.
fn mru_pane(items: &[Item], rows: &[Row], row: usize) -> Option<usize> {
    let depth = rows[row].depth;
    rows[row + 1..]
        .iter()
        .take_while(|r| r.depth > depth)
        .filter_map(Row::item)
        .min_by_key(|&i| Reverse(items[i].times.last_focused_ms))
}

/// Sort key of a group line, computed from every pane under it: the most
/// urgent agent status (groups without agents last), then the latest focus,
/// then the latest status change, then herdr's number or the label.
type GroupKey = (u8, Reverse<u64>, Reverse<u64>, (u64, String));

/// Tier of a group with no agent panes: after every agent tier.
const NO_AGENTS: u8 = Status::Unknown as u8 + 1;

struct Group {
    node: Node,
    summary: Summary,
    key: GroupKey,
    children: Vec<Group>,
    /// Pane rows directly under this group, for groups without children.
    panes: Vec<usize>,
}

impl Group {
    /// `under` is every pane under the group, at any depth.
    fn new(
        node: Node,
        label: String,
        rank: (u64, String),
        under: &[usize],
        items: &[Item],
    ) -> Group {
        let mut summary = Summary {
            label,
            ..Default::default()
        };
        let (mut tier, mut focused_ms, mut changed_ms) = (NO_AGENTS, 0, 0);
        for item in under.iter().map(|&i| &items[i]) {
            focused_ms = focused_ms.max(item.times.last_focused_ms);
            changed_ms = changed_ms.max(item.times.last_status_change_ms);
            summary.last_activity_ms = summary.last_activity_ms.max(item.times.last_activity_ms());
            if item.is_agent() {
                tier = tier.min(item.status.tier());
                if let Some(slot) = BADGES.iter().position(|&s| s == item.status) {
                    summary.counts[slot] += 1;
                }
            }
        }
        Group {
            node,
            summary,
            key: (tier, Reverse(focused_ms), Reverse(changed_ms), rank),
            children: Vec::new(),
            panes: Vec::new(),
        }
    }

    fn with_panes(mut self, mut panes: Vec<usize>, items: &[Item]) -> Group {
        panes.sort_by(|&a, &b| tier_mru_cmp(&items[a], &items[b]));
        self.panes = panes;
        self
    }

    fn with_children(mut self, mut children: Vec<Group>) -> Group {
        sort_groups(&mut children);
        self.children = children;
        self
    }

    fn any_kept(&self, keep: &impl Fn(usize) -> bool) -> bool {
        self.panes.iter().any(|&i| keep(i)) || self.children.iter().any(|c| c.any_kept(keep))
    }
}

fn sort_groups(groups: &mut [Group]) {
    groups.sort_by(|a, b| a.key.cmp(&b.key));
}

/// Groups `panes` by `key`, keeping first-seen order.
fn group_by<K: Eq + Hash + Clone>(
    panes: impl IntoIterator<Item = usize>,
    key: impl Fn(usize) -> K,
) -> Vec<(K, Vec<usize>)> {
    let mut index: HashMap<K, usize> = HashMap::new();
    let mut groups: Vec<(K, Vec<usize>)> = Vec::new();
    for i in panes {
        let k = key(i);
        match index.get(&k) {
            Some(&g) => groups[g].1.push(i),
            None => {
                index.insert(k.clone(), groups.len());
                groups.push((k, vec![i]));
            }
        }
    }
    groups
}

/// Depth-first, dropping groups with no kept pane under them.
fn flatten(
    groups: Vec<Group>,
    depth: u8,
    keep: &impl Fn(usize) -> bool,
    matched: &impl Fn(usize) -> Option<MatchResult>,
    out: &mut Vec<Row>,
) {
    for group in groups {
        if !group.any_kept(keep) {
            continue;
        }
        out.push(Row {
            node: group.node,
            depth,
            matched: None,
            summary: Some(group.summary),
        });
        flatten(group.children, depth + 1, keep, matched, out);
        out.extend(
            group
                .panes
                .into_iter()
                .filter(|&i| keep(i))
                .map(|i| pane_row(i, depth + 1, matched(i))),
        );
    }
}

fn unknown_or(label: &str) -> String {
    if label.is_empty() {
        UNKNOWN.into()
    } else {
        label.into()
    }
}

/// Workspace, then tab, then pane.
fn workspaces_tree(items: &[Item]) -> Vec<Group> {
    let mut workspaces: Vec<Group> = group_by(0..items.len(), |i| items[i].workspace_id.clone())
        .into_iter()
        .map(|(id, under)| {
            let first = &items[under[0]];
            let tabs = group_by(under.iter().copied(), |i| items[i].tab_id.clone())
                .into_iter()
                .map(|(tab_id, panes)| {
                    let first = &items[panes[0]];
                    let rank = (first.tab_number.unwrap_or(u64::MAX), String::new());
                    Group::new(Node::Tab(tab_id), first.tab_line(), rank, &panes, items)
                        .with_panes(panes, items)
                })
                .collect();
            let rank = (first.workspace_number.unwrap_or(u64::MAX), String::new());
            Group::new(
                Node::Workspace(id),
                unknown_or(&first.workspace),
                rank,
                &under,
                items,
            )
            .with_children(tabs)
        })
        .collect();
    sort_groups(&mut workspaces);
    workspaces
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum ProjectKey {
    Repo(PathBuf),
    Folder(String),
}

/// Repo, then worktree (skipped when the repo has one), then pane; panes
/// outside a repo go under folder lines.
fn projects_tree(items: &[Item], home: &str) -> Vec<Group> {
    let key = |i: usize| match &items[i].git {
        Some(git) => ProjectKey::Repo(git.common_dir.clone()),
        None => ProjectKey::Folder(items[i].cwd.clone()),
    };
    let mut projects: Vec<Group> = group_by(0..items.len(), key)
        .into_iter()
        .map(|(key, under)| match key {
            ProjectKey::Repo(dir) => repo_group(dir, under, items),
            ProjectKey::Folder(cwd) => {
                let label = unknown_or(&tilde(&cwd, home));
                let rank = (0, label.clone());
                Group::new(Node::Folder(cwd), label, rank, &under, items).with_panes(under, items)
            }
        })
        .collect();
    sort_groups(&mut projects);
    projects
}

fn repo_group(dir: PathBuf, under: Vec<usize>, items: &[Item]) -> Group {
    let git = |i: usize| items[i].git.as_ref().expect("repo panes have git info");
    let branch = |i: usize| {
        let g = git(i);
        if g.branch.is_empty() {
            g.project.clone()
        } else {
            g.branch.clone()
        }
    };
    let label = git(under[0]).repo.clone();
    let rank = (0, label.clone());
    let mut worktrees = group_by(under.iter().copied(), |i| git(i).worktree_root.clone());
    let repo = Group::new(Node::Repo(dir), label, rank, &under, items);
    if worktrees.len() == 1 {
        let (_, panes) = worktrees.remove(0);
        let mut repo = repo.with_panes(panes, items);
        repo.summary.detail = Some(branch(under[0])).filter(|b| !b.is_empty());
        return repo;
    }
    let children = worktrees
        .into_iter()
        .map(|(root, panes)| {
            let branch = branch(panes[0]);
            Group::new(
                Node::Worktree(root),
                format!("⎇ {branch}"),
                (0, branch),
                &panes,
                items,
            )
            .with_panes(panes, items)
        })
        .collect();
    repo.with_children(children)
}

/// `path` with a leading `home` replaced by `~`.
fn tilde(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return path.into();
    }
    match Path::new(path).strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{git::GitInfo, search::Searcher, state::PaneTimes};

    fn pane(id: &str, ws: u64, tab: u64, agent: Option<&str>, status: Status) -> Item {
        Item {
            pane_id: id.into(),
            workspace_id: format!("w{ws}"),
            tab_id: format!("w{ws}:t{tab}"),
            workspace: format!("ws{ws}"),
            tab: format!("tab {tab}"),
            workspace_number: Some(ws),
            tab_number: Some(tab),
            agent: agent.map(str::to_string),
            status,
            title: format!("title {id}"),
            ..Default::default()
        }
    }

    fn focused_at(mut item: Item, ms: u64) -> Item {
        item.times = PaneTimes {
            last_focused_ms: ms,
            ..Default::default()
        };
        item
    }

    fn in_repo(mut item: Item, repo: &str, root: &str, branch: &str) -> Item {
        item.cwd = root.into();
        item.git = Some(GitInfo {
            project: crate::model::basename(root).into(),
            repo: repo.into(),
            branch: branch.into(),
            common_dir: PathBuf::from(format!("/git/{repo}")),
            worktree_root: PathBuf::from(root),
            ..Default::default()
        });
        item
    }

    /// One line per row: indent, then the pane id or the group label.
    fn outline(items: &[Item], rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| {
                let indent = "  ".repeat(r.depth as usize);
                match (&r.node, &r.summary) {
                    (Node::Pane(i), _) => format!("{indent}{}", items[*i].pane_id),
                    (_, Some(s)) => match &s.detail {
                        Some(d) => format!("{indent}{} [{d}]", s.label),
                        None => format!("{indent}{}", s.label),
                    },
                    _ => unreachable!(),
                }
            })
            .collect()
    }

    fn rows(view: View, items: &[Item]) -> Vec<String> {
        outline(items, &build_rows(view, items, None, None))
    }

    fn matches(items: &[Item], query: &str) -> Vec<Option<MatchResult>> {
        let mut s = Searcher::default();
        s.set_query(query);
        items.iter().map(|i| s.match_item(i)).collect()
    }

    #[test]
    fn views_cycle_and_wrap() {
        assert_eq!(View::Agents.next(), View::Workspaces);
        assert_eq!(View::Projects.next(), View::Agents);
        assert_eq!(View::Agents.prev(), View::Projects);
        assert_eq!(View::parse("projects"), Some(View::Projects));
    }

    #[test]
    fn workspaces_group_by_workspace_then_tab() {
        let items = vec![
            focused_at(pane("a", 1, 1, Some("claude"), Status::Idle), 5),
            pane("shell", 1, 1, None, Status::Unknown),
            pane("b", 1, 2, Some("codex"), Status::Idle),
            pane("c", 2, 1, Some("pi"), Status::Idle),
        ];
        assert_eq!(
            rows(View::Workspaces, &items),
            [
                "ws1",
                "  1",
                "    a",
                "    shell",
                "  2",
                "    b",
                "ws2",
                "  1",
                "    c"
            ]
        );
    }

    #[test]
    fn blocked_agent_pulls_its_groups_up() {
        let items = vec![
            focused_at(pane("idle", 1, 1, Some("claude"), Status::Idle), 50),
            pane("quiet", 2, 1, Some("claude"), Status::Idle),
            pane("blocked", 2, 2, Some("claude"), Status::Blocked),
        ];
        assert_eq!(
            rows(View::Workspaces, &items),
            [
                "ws2",
                "  2",
                "    blocked",
                "  1",
                "    quiet",
                "ws1",
                "  1",
                "    idle"
            ]
        );
    }

    #[test]
    fn focus_time_breaks_ties_within_a_tier() {
        let items = vec![
            focused_at(pane("old", 1, 1, Some("claude"), Status::Idle), 10),
            focused_at(pane("new", 2, 1, Some("claude"), Status::Idle), 20),
            pane("never", 3, 1, Some("claude"), Status::Idle),
            focused_at(pane("shell", 4, 1, None, Status::Unknown), 99),
        ];
        let ws: Vec<String> = rows(View::Workspaces, &items)
            .into_iter()
            .filter(|r| !r.starts_with(' '))
            .collect();
        assert_eq!(
            ws,
            ["ws2", "ws1", "ws3", "ws4"],
            "groups without agents sort last"
        );
    }

    #[test]
    fn group_summary_counts_agents_and_latest_activity() {
        let items = vec![
            focused_at(pane("a", 1, 1, Some("claude"), Status::Blocked), 7),
            pane("b", 1, 1, Some("claude"), Status::Working),
            pane("c", 1, 1, Some("claude"), Status::Working),
            pane("d", 1, 1, Some("claude"), Status::Unknown),
            focused_at(pane("shell", 1, 1, None, Status::Unknown), 9),
        ];
        let built = build_rows(View::Workspaces, &items, None, None);
        let summary = built[0].summary.as_ref().unwrap();
        assert_eq!(summary.counts, [1, 0, 2, 0]);
        assert_eq!(summary.last_activity_ms, 9);
    }

    #[test]
    fn query_and_filter_prune_but_keep_order() {
        let items = vec![
            pane("blocked", 1, 1, Some("claude"), Status::Blocked),
            pane("idle", 1, 2, Some("claude"), Status::Idle),
            pane("shell", 2, 1, None, Status::Unknown),
        ];
        let filtered = build_rows(View::Workspaces, &items, None, Some(Status::Idle));
        assert_eq!(
            outline(&items, &filtered),
            ["ws1", "  2", "    idle"],
            "plain shells never pass a status filter"
        );

        let m = matches(&items, "ws2");
        let queried = build_rows(View::Workspaces, &items, Some(&m), None);
        assert_eq!(outline(&items, &queried), ["ws2", "  1", "    shell"]);

        let m = matches(&items, "title");
        let queried = build_rows(View::Workspaces, &items, Some(&m), None);
        assert_eq!(
            outline(&items, &queried),
            rows(View::Workspaces, &items),
            "aggregate order, not score order"
        );
    }

    #[test]
    fn focused_pane_only_in_trees() {
        let mut focused = in_repo(
            pane("here", 1, 1, Some("claude"), Status::Idle),
            "r",
            "/r",
            "main",
        );
        focused.focused = true;
        let items = vec![focused, pane("other", 1, 1, Some("codex"), Status::Idle)];
        assert_eq!(rows(View::Agents, &items), ["other"]);
        assert!(rows(View::Workspaces, &items).contains(&"    here".to_string()));
        assert!(rows(View::Projects, &items).contains(&"  here".to_string()));
    }

    #[test]
    fn projects_group_by_repo_identity_and_folder() {
        let mut items = vec![
            in_repo(
                pane("a", 1, 1, Some("claude"), Status::Idle),
                "api",
                "/x/api",
                "main",
            ),
            in_repo(
                pane("b", 2, 1, Some("codex"), Status::Idle),
                "api",
                "/y/api",
                "dev",
            ),
            pane("notes", 1, 2, None, Status::Unknown),
            pane("nowhere", 1, 3, None, Status::Unknown),
        ];
        // Same label, different repo: stays separate.
        items[1].git.as_mut().unwrap().common_dir = PathBuf::from("/git/other-api");
        items[2].cwd = "/home/me/notes".into();
        let rows = outline(&items, &build_rows(View::Projects, &items, None, None));
        assert_eq!(
            rows,
            [
                "api [main]",
                "  a",
                "api [dev]",
                "  b",
                "(unknown)",
                "  nowhere",
                "/home/me/notes",
                "  notes",
            ]
        );
        assert_eq!(tilde("/home/me/notes", "/home/me"), "~/notes");
        assert_eq!(tilde("/home/me", "/home/me/"), "~");
        assert_eq!(tilde("/home/mel", "/home/me"), "/home/mel");
    }

    #[test]
    fn worktree_level_only_with_several_checkouts() {
        let items = vec![
            focused_at(
                in_repo(
                    pane("a", 1, 1, Some("claude"), Status::Idle),
                    "r",
                    "/r",
                    "main",
                ),
                1,
            ),
            in_repo(
                pane("b", 1, 1, Some("codex"), Status::Working),
                "r",
                "/r-wt",
                "feat",
            ),
            in_repo(pane("c", 1, 1, None, Status::Unknown), "r", "/r", "main"),
        ];
        assert_eq!(
            rows(View::Projects, &items),
            ["r", "  ⎇ feat", "    b", "  ⎇ main", "    a", "    c"]
        );
        assert_eq!(
            rows(View::Projects, &items[..1]),
            ["r [main]", "  a"],
            "one checkout skips the middle level"
        );
    }

    #[test]
    fn targets_resolve_each_node_kind() {
        let mut items = vec![
            in_repo(
                pane("agent", 1, 1, Some("claude"), Status::Idle),
                "r",
                "/r",
                "main",
            ),
            focused_at(
                in_repo(
                    pane("shell", 1, 2, None, Status::Unknown),
                    "r",
                    "/r",
                    "main",
                ),
                9,
            ),
            in_repo(
                pane("here", 1, 1, Some("pi"), Status::Done),
                "r",
                "/r",
                "main",
            ),
        ];
        items[2].focused = true;
        let find = |rows: &[Row], node: Node| rows.iter().position(|r| r.node == node).unwrap();

        let ws = build_rows(View::Workspaces, &items, None, None);
        let t = |node| target(&items, &ws, find(&ws, node));
        assert_eq!(
            t(Node::Workspace("w1".into())),
            Some(Target::Workspace("w1".into()))
        );
        assert_eq!(
            t(Node::Tab("w1:t2".into())),
            Some(Target::Tab("w1:t2".into()))
        );
        assert_eq!(t(Node::Pane(0)), Some(Target::Agent("agent".into())));
        assert_eq!(t(Node::Pane(1)), Some(Target::Pane("shell".into())));
        assert_eq!(t(Node::Pane(2)), Some(Target::Close));

        let pr = build_rows(View::Projects, &items, None, None);
        let repo = find(&pr, Node::Repo("/git/r".into()));
        assert_eq!(
            target(&items, &pr, repo),
            Some(Target::Pane("shell".into())),
            "most recently used pane under the line"
        );
        items[1].times.last_focused_ms = 0;
        let pr = build_rows(View::Projects, &items, None, None);
        assert_eq!(
            target(&items, &pr, repo),
            Some(Target::Close),
            "ties go to the first pane row"
        );
    }

    #[test]
    fn worktree_and_folder_lines_target_their_mru_pane() {
        let items = vec![
            in_repo(
                pane("a", 1, 1, Some("claude"), Status::Idle),
                "r",
                "/r",
                "main",
            ),
            focused_at(
                in_repo(
                    pane("b", 1, 1, Some("codex"), Status::Idle),
                    "r",
                    "/r-wt",
                    "feat",
                ),
                3,
            ),
            focused_at(pane("old", 1, 1, None, Status::Unknown), 1),
            focused_at(pane("new", 1, 1, None, Status::Unknown), 2),
        ];
        let rows = build_rows(View::Projects, &items, None, None);
        let at = |node: Node| rows.iter().position(|r| r.node == node).unwrap();
        assert_eq!(
            target(&items, &rows, at(Node::Worktree("/r-wt".into()))),
            Some(Target::Agent("b".into()))
        );
        assert_eq!(
            target(&items, &rows, at(Node::Folder(String::new()))),
            Some(Target::Pane("new".into()))
        );
    }

    #[test]
    fn unknown_workspace_falls_back_to_a_pane() {
        let mut item = pane("lost", 1, 1, None, Status::Unknown);
        item.workspace.clear();
        let items = vec![item];
        let rows = build_rows(View::Workspaces, &items, None, None);
        assert_eq!(rows[0].summary.as_ref().unwrap().label, UNKNOWN);
        assert_eq!(target(&items, &rows, 0), Some(Target::Pane("lost".into())));
    }
}
