//! Multi-field fuzzy search.
//!
//! The query is split fzf-style into space-separated atoms (nucleo syntax:
//! `'exact`, `^prefix`, `suffix$`, `!negated`). Every atom must match at
//! least one field of a row (AND across atoms, OR across fields); a row's
//! score is the sum of each atom's best weighted field score.

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{Atom, CaseMatching, Normalization, Pattern},
};

use crate::model::Item;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Agent,
    Status,
    Title,
    Tab,
    Workspace,
    Folder,
    Repo,
    Project,
    Branch,
    Worktree,
    Path,
}

impl Field {
    /// Weight in percent: agent / title / tab / workspace above repo /
    /// branch above the full path.
    fn weight(self) -> u32 {
        match self {
            Field::Agent | Field::Status | Field::Title | Field::Tab | Field::Workspace => 100,
            Field::Folder | Field::Repo | Field::Project | Field::Branch | Field::Worktree => 75,
            Field::Path => 50,
        }
    }

    /// Fields the row shows (agent as its logo, status as its dot);
    /// matches elsewhere get a hint line.
    pub fn is_visible(self) -> bool {
        matches!(
            self,
            Field::Agent
                | Field::Status
                | Field::Title
                | Field::Tab
                | Field::Workspace
                | Field::Folder
                | Field::Project
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Field::Agent => "agent",
            Field::Status => "status",
            Field::Title => "title",
            Field::Tab => "tab",
            Field::Workspace => "workspace",
            Field::Folder => "folder",
            Field::Repo => "repo",
            Field::Project => "project",
            Field::Branch => "branch",
            Field::Worktree => "worktree",
            Field::Path => "path",
        }
    }
}

/// The searchable text of each field of an item.
pub fn fields(item: &Item) -> Vec<(Field, &str)> {
    let mut out = vec![
        (Field::Agent, item.agent.as_str()),
        (Field::Status, item.status.word()),
        (Field::Title, item.title.as_str()),
        (Field::Tab, item.tab.as_str()),
        (Field::Workspace, item.workspace.as_str()),
    ];
    if let Some(git) = &item.git {
        out.push((Field::Project, git.project.as_str()));
        out.push((Field::Repo, git.repo.as_str()));
        out.push((Field::Branch, git.branch.as_str()));
        if let Some(wt) = &git.worktree {
            out.push((Field::Worktree, wt.as_str()));
        }
    } else {
        out.push((Field::Folder, crate::model::basename(&item.cwd)));
    }
    out.push((Field::Path, item.cwd.as_str()));
    out.retain(|(_, text)| !text.is_empty());
    out
}

/// Matched char indices for one field of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMatch {
    pub field: Field,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct MatchResult {
    pub score: u32,
    pub matches: Vec<FieldMatch>,
}

impl MatchResult {
    pub fn indices(&self, field: Field) -> &[u32] {
        self.matches
            .iter()
            .find(|m| m.field == field)
            .map(|m| m.indices.as_slice())
            .unwrap_or(&[])
    }

    /// The best-scoring hidden field, if any atom matched best outside the
    /// visible row text.
    pub fn hidden_hit(&self) -> Option<Field> {
        self.matches
            .iter()
            .map(|m| m.field)
            .find(|f| !f.is_visible())
    }
}

pub struct Searcher {
    matcher: Matcher,
    pattern: Pattern,
    buf: Vec<char>,
}

impl Default for Searcher {
    fn default() -> Self {
        Searcher {
            matcher: Matcher::new(Config::DEFAULT),
            pattern: Pattern::default(),
            buf: Vec::new(),
        }
    }
}

impl Searcher {
    pub fn set_query(&mut self, query: &str) {
        self.pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    }

    pub fn is_empty(&self) -> bool {
        self.pattern.atoms.is_empty()
    }

    /// Returns `None` when the row does not match every atom.
    pub fn match_item(&mut self, item: &Item) -> Option<MatchResult> {
        let fields = fields(item);
        let mut result = MatchResult::default();
        for atom in &self.pattern.atoms {
            if atom.negative {
                let excluded = fields.iter().any(|(_, text)| {
                    atom.score(Utf32Str::new(text, &mut self.buf), &mut self.matcher)
                        .is_none()
                });
                if excluded {
                    return None;
                }
                continue;
            }
            let (score, hit) = best_field(atom, &fields, &mut self.matcher, &mut self.buf)?;
            result.score += score;
            merge(&mut result.matches, hit);
        }
        // Visible fields first so callers can render them; hidden after.
        result.matches.sort_by_key(|m| !m.field.is_visible());
        Some(result)
    }
}

fn best_field(
    atom: &Atom,
    fields: &[(Field, &str)],
    matcher: &mut Matcher,
    buf: &mut Vec<char>,
) -> Option<(u32, FieldMatch)> {
    let mut best: Option<(u32, FieldMatch)> = None;
    let mut indices = Vec::new();
    for &(field, text) in fields {
        indices.clear();
        let Some(score) = atom.indices(Utf32Str::new(text, buf), matcher, &mut indices) else {
            continue;
        };
        let weighted = u32::from(score) * field.weight() / 100;
        if best.as_ref().is_none_or(|(b, _)| weighted > *b) {
            best = Some((
                weighted,
                FieldMatch {
                    field,
                    indices: indices.clone(),
                },
            ));
        }
    }
    best
}

fn merge(matches: &mut Vec<FieldMatch>, hit: FieldMatch) {
    match matches.iter_mut().find(|m| m.field == hit.field) {
        Some(existing) => {
            existing.indices.extend(hit.indices);
            existing.indices.sort_unstable();
            existing.indices.dedup();
        }
        None => matches.push(hit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        git::GitInfo,
        model::{Harness, Status},
        state::PaneTimes,
    };

    fn item(title: &str, agent: &str, status: Status, cwd: &str, branch: &str) -> Item {
        Item {
            pane_id: title.into(),
            harness: Harness::detect(agent),
            agent: agent.into(),
            status,
            title: title.into(),
            workspace: "work".into(),
            tab: "tab".into(),
            cwd: cwd.into(),
            git: (!branch.is_empty()).then(|| GitInfo {
                project: crate::model::basename(cwd).into(),
                repo: "herdr-blink".into(),
                branch: branch.into(),
                worktree: None,
            }),
            times: PaneTimes::default(),
            order: 0,
        }
    }

    fn search(query: &str, item: &Item) -> Option<MatchResult> {
        let mut s = Searcher::default();
        s.set_query(query);
        s.match_item(item)
    }

    #[test]
    fn terms_are_anded_across_fields() {
        let it = item(
            "Fix login",
            "claude",
            Status::Blocked,
            "/src/app",
            "feat/auth",
        );
        assert!(search("claude login", &it).is_some());
        assert!(search("blocked auth", &it).is_some());
        assert!(search("codex login", &it).is_none());
        assert!(search("!codex login", &it).is_some());
        assert!(search("!claude", &it).is_none());
    }

    #[test]
    fn title_outranks_path() {
        let a = item("deploy", "claude", Status::Idle, "/x/y", "");
        let b = item("other", "claude", Status::Idle, "/x/deploy/y", "");
        let sa = search("deploy", &a).unwrap().score;
        let sb = search("deploy", &b).unwrap().score;
        assert!(sa > sb, "{sa} <= {sb}");
    }

    #[test]
    fn reports_hidden_field_hits() {
        let it = item(
            "Fix login",
            "claude",
            Status::Idle,
            "/src/app",
            "feat/oauth-flow",
        );
        let r = search("oauth", &it).unwrap();
        assert_eq!(r.hidden_hit(), Some(Field::Branch));
        let r = search("login", &it).unwrap();
        assert_eq!(r.hidden_hit(), None);
        assert_eq!(r.indices(Field::Title), &[4, 5, 6, 7, 8]);
    }
}
