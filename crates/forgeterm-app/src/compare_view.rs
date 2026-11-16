//! Compare Worktrees: two worktrees side by side — what each changed since their
//! common commit, a side-by-side diff of one file between them, test terminals
//! for both, and one prompt for both agents. Facts only: counts, exit codes and
//! their limits; no verdict on which side is better.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use forgeterm_agents::AgentKind;
use forgeterm_git::{ChangedFile, Row, RowKind, SideBySide, Worktree};
use iced::widget::{
    column, container, mouse_area, pick_list, row, scrollable, space, text, text_editor, text_input,
};
use iced::{Border, Color, Element, Length};

use crate::app::Message;
use crate::commands::CommandId;
use crate::git_dash::{
    self, card, card_title, diff_bar, message_card, muted, muted_owned, plural, tile, untrusted,
};
use crate::git_view::Repo;
use crate::mission_view::{action_button, agent_badge, tag, tail, with_tip};
use crate::terminal_pane::TermId;
use crate::terminal_view::TERMINAL_FONT;
use crate::theme;

pub const TEST_INPUT_ID: &str = "compare-test-command";
/// Rows of a side-by-side diff that are drawn; the rest is summarized.
const MAX_ROWS_SHOWN: usize = 2000;
/// A test terminal that shows no prompt within this time gets no inserted command.
pub const INSERT_GIVE_UP: Duration = Duration::from_secs(20);
/// Without shell integration: output quiet this long counts as "at the prompt".
const QUIET_FOR_INSERT: Duration = Duration::from_millis(1500);

#[derive(Debug, Default)]
pub struct Compare {
    pub visible: bool,
    pub busy: bool,
    /// The worktrees chosen for side A and B (`None`: the default pick).
    pub a: Option<PathBuf>,
    pub b: Option<PathBuf>,
    pub data: Option<CompareData>,
    /// Bumped by every load: an older result is dropped.
    pub seq: u64,
    pub selected: Option<String>,
    pub diff: Option<DiffState>,
    pub test_command: String,
    /// The user typed a command: a reload no longer replaces it.
    pub test_command_edited: bool,
    /// Terminals opened by "Open test terminals", per side.
    pub test_terms: [Option<TermId>; 2],
    pub prompt: text_editor::Content,
}

#[derive(Debug, Clone)]
pub struct DiffState {
    pub path: String,
    /// `None` while it loads.
    pub result: Option<Result<(SideBySide, bool), String>>,
}

/// One background load (see [`load`]).
#[derive(Debug, Clone)]
pub struct CompareData {
    pub seq: u64,
    pub root: PathBuf,
    pub worktrees: Result<Vec<Worktree>, String>,
    /// `None`: fewer than two worktrees to compare.
    pub sides: Option<[Side; 2]>,
    /// The common commit, or why there is none.
    pub base: Result<String, String>,
    /// [`file_rows`] of the two sides (empty when a side was not read).
    pub rows: Vec<FileRow>,
    pub test_command: Option<&'static str>,
    pub loaded_at: Instant,
}

#[derive(Debug, Clone)]
pub struct Side {
    pub worktree: Worktree,
    /// What changed since the base, or why it was not read.
    pub files: Result<Vec<ChangedFile>, String>,
}

impl Compare {
    /// Side A's and B's folders of the loaded data.
    pub fn paths(&self) -> Option<[PathBuf; 2]> {
        let [a, b] = self.data.as_ref()?.sides.as_ref()?;
        Some([a.worktree.path.clone(), b.worktree.path.clone()])
    }
}

/// A worktree in the side pickers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub path: PathBuf,
    pub label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

fn usable(w: &Worktree) -> bool {
    !w.bare && w.head.is_some() && w.prunable.is_none()
}

fn same(a: &Path, b: &Path) -> bool {
    git_dash::path_key(a) == git_dash::path_key(b)
}

/// Which worktrees to compare: the chosen ones when they are still there, else
/// the first two linked worktrees (two agents), else the main one and the linked
/// one. Never the same worktree twice; `None` with fewer than two.
pub fn pick_sides(list: &[Worktree], a: Option<&Path>, b: Option<&Path>) -> Option<(usize, usize)> {
    let ok: Vec<usize> = (0..list.len()).filter(|&i| usable(&list[i])).collect();
    if ok.len() < 2 {
        return None;
    }
    let find =
        |p: Option<&Path>| p.and_then(|p| ok.iter().copied().find(|&i| same(&list[i].path, p)));
    let linked: Vec<usize> = ok.iter().copied().filter(|&i| i > 0).collect();
    let default = if linked.len() >= 2 {
        (linked[0], linked[1])
    } else {
        (ok[0], ok[1])
    };
    let ia = find(a).unwrap_or(default.0);
    let ib = find(b).filter(|&i| i != ia).unwrap_or_else(|| {
        [default.1, default.0]
            .into_iter()
            .chain(ok.iter().copied())
            .find(|&i| i != ia)
            .unwrap_or(default.1)
    });
    Some((ia, ib))
}

/// A file in the list: its index in A's and B's files (`None`: unchanged there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    pub path: String,
    pub a: Option<usize>,
    pub b: Option<usize>,
}

impl FileRow {
    pub fn in_both(&self) -> bool {
        self.a.is_some() && self.b.is_some()
    }
}

/// Every path changed in A or B, sorted. Built once per load (a worktree with an
/// un-ignored `node_modules` can list tens of thousands of files).
pub fn file_rows(a: &[ChangedFile], b: &[ChangedFile]) -> Vec<FileRow> {
    let mut map: std::collections::BTreeMap<&str, (Option<usize>, Option<usize>)> =
        std::collections::BTreeMap::new();
    for (i, f) in a.iter().enumerate() {
        map.entry(f.path.as_str()).or_default().0 = Some(i);
    }
    for (i, f) in b.iter().enumerate() {
        map.entry(f.path.as_str()).or_default().1 = Some(i);
    }
    map.into_iter()
        .map(|(path, (a, b))| FileRow {
            path: path.to_owned(),
            a,
            b,
        })
        .collect()
}

/// Whether a new shell is ready for an inserted command: it showed its prompt
/// (shell integration), or — for shells that do not mark prompts — its output has
/// been quiet for a moment.
pub fn ready_for_insert(at_prompt: bool, since_output: Option<Duration>) -> bool {
    at_prompt || since_output.is_some_and(|d| d >= QUIET_FOR_INSERT)
}

/// Added and deleted lines over all files (binary and uncounted files add 0).
fn totals(files: &[ChangedFile]) -> (u32, u32) {
    files.iter().fold((0, 0), |(a, d), f| {
        (a + f.added.unwrap_or(0), d + f.deleted.unwrap_or(0))
    })
}

/// Reads everything for the page (background thread). `trusted`: worktrees whose
/// own config runs programs but the user trusted.
pub fn load(
    seq: u64,
    root: PathBuf,
    a: Option<PathBuf>,
    b: Option<PathBuf>,
    trusted: &[PathBuf],
) -> CompareData {
    let worktrees = forgeterm_git::worktrees(&root);
    let mut base = Err("fewer than two worktrees".to_owned());
    let mut sides = None;
    if let Ok(list) = &worktrees
        && let Some((ia, ib)) = pick_sides(list, a.as_deref(), b.as_deref())
    {
        let (wa, wb) = (list[ia].clone(), list[ib].clone());
        base = match (&wa.head, &wb.head) {
            (Some(ha), Some(hb)) => forgeterm_git::merge_base(&root, ha, hb),
            _ => Err("a worktree has no commit".to_owned()),
        };
        let read = |w: &Worktree| -> Result<Vec<ChangedFile>, String> {
            let base = base.as_ref().map_err(Clone::clone)?;
            read_side(&w.path, base, trusted)
        };
        let (fa, fb) = std::thread::scope(|s| {
            let ha = s.spawn(|| read(&wa));
            let hb = s.spawn(|| read(&wb));
            let join = |h: std::thread::ScopedJoinHandle<'_, _>| {
                h.join()
                    .unwrap_or_else(|_| Err("reading it failed unexpectedly".to_owned()))
            };
            (join(ha), join(hb))
        });
        sides = Some([
            Side {
                worktree: wa,
                files: fa,
            },
            Side {
                worktree: wb,
                files: fb,
            },
        ]);
    }
    let rows = match &sides {
        Some([Side { files: Ok(a), .. }, Side { files: Ok(b), .. }]) => file_rows(a, b),
        _ => Vec::new(),
    };
    let test_command = sides
        .as_ref()
        .and_then(|[a, _]| forgeterm_agents::test_command(&a.worktree.path))
        .or_else(|| forgeterm_agents::test_command(&root));
    CompareData {
        seq,
        root,
        worktrees,
        sides,
        base,
        rows,
        test_command,
        loaded_at: Instant::now(),
    }
}

fn read_side(path: &Path, base: &str, trusted: &[PathBuf]) -> Result<Vec<ChangedFile>, String> {
    if !path.to_str().is_some_and(forgeterm_terminal::is_local_path)
        || !forgeterm_fs::resolves_locally(path)
    {
        return Err("network folder: not read".into());
    }
    if !path.is_dir() {
        return Err("folder not found".into());
    }
    let programs = forgeterm_git::repo_programs(path)?;
    if !programs.is_empty() && !trusted.iter().any(|t| same(t, path)) {
        return Err("not read: its config runs programs (trust it in the Git panel)".into());
    }
    forgeterm_git::changes_since(path, base)
}

/// Side-by-side diff of `rel` between the two folders (background thread).
pub fn load_diff(root: &Path, a: &Path, b: &Path, rel: &str) -> Result<(SideBySide, bool), String> {
    let side = |dir: &Path| -> Result<Option<PathBuf>, String> {
        let p = forgeterm_git::safe_join(dir, rel)
            .ok_or("this path is not shown (it leads outside the worktree)")?;
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_file() || m.file_type().is_symlink() => Ok(Some(p)),
            Ok(_) => Err("not a regular file".to_owned()),
            Err(_) => Ok(None),
        }
    };
    let (pa, pb) = (side(a)?, side(b)?);
    if pa.is_none() && pb.is_none() {
        return Err("the file exists in neither worktree (deleted in both)".into());
    }
    let d = forgeterm_git::diff_files(root, pa.as_deref(), pb.as_deref())?;
    Ok((forgeterm_git::side_by_side(&d.lines), d.truncated))
}

/// What the test terminal of one side shows, from shell integration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestState {
    /// No test terminal opened (or it was closed).
    None,
    /// Waiting for the shell's prompt to insert the command.
    Inserting,
    Ended,
    /// At the prompt, nothing ran yet: the user presses Enter.
    NothingYet,
    Running(Option<String>),
    Finished(Option<String>, Option<i32>),
    /// The shell does not report commands: the result is only in the terminal.
    NotReported,
}

/// An agent working in one of the two worktrees.
#[derive(Debug, Clone)]
pub struct AgentIn {
    pub id: TermId,
    pub kind: AgentKind,
    pub tab: usize,
}

pub struct View<'a> {
    pub repo: Option<&'a Repo>,
    pub compare: &'a Compare,
    pub agents: [Vec<AgentIn>; 2],
    pub tests: [(Option<TermId>, TestState); 2],
    pub now: Instant,
}

pub fn view(v: View<'_>) -> Element<'_, Message> {
    let body: Element<'_, Message> = match v.repo {
        None => message_card("Reading the repository…", None),
        Some(Repo::Outside) => message_card(
            "The focused terminal is not in a Git repository",
            Some("cd into a repository with worktrees and this page follows it.".to_owned()),
        ),
        Some(Repo::Error(e)) => message_card("git reported an error", Some(e.clone())),
        Some(Repo::Untrusted { root, programs }) => untrusted(root, programs),
        Some(Repo::Found { root, .. }) => match v.compare.data.as_ref().filter(|d| &d.root == root)
        {
            None => message_card("Reading the worktrees…", None),
            Some(data) => found(data, &v),
        },
    };
    container(
        scrollable(container(body).padding([18, 22]))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(6).scroller_width(6),
            ))
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::obsidian().into()),
        ..Default::default()
    })
    .into()
}

fn name_of(w: &Worktree) -> String {
    w.path.file_name().map_or_else(
        || w.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn choice(w: &Worktree) -> Choice {
    Choice {
        path: w.path.clone(),
        label: match &w.branch {
            Some(b) => format!("{} · {b}", name_of(w)),
            None => name_of(w),
        },
    }
}

fn found<'a>(data: &'a CompareData, v: &View<'a>) -> Element<'a, Message> {
    let list = match &data.worktrees {
        Err(e) => return message_card("git could not list the worktrees", Some(e.clone())),
        Ok(l) => l,
    };
    let Some([a, b]) = &data.sides else {
        return container(
            column![
                message_card(
                    "Compare needs two worktrees",
                    Some(
                        "This repository has one. Start an agent in a new worktree (each gets its \
                         own branch and folder), then compare its work with yours or another agent's."
                            .to_owned()
                    ),
                ),
                action_button(
                    "Agent in new worktree…".to_owned(),
                    Some(Message::Command(CommandId::NewAgentWorktree)),
                    theme::cyan(),
                    true,
                ),
            ]
            .spacing(theme::space::XL),
        )
        .into();
    };
    let choices: Vec<Choice> = list.iter().filter(|w| usable(w)).map(choice).collect();
    let mut page = column![header(data, a, b, choices, v)].spacing(theme::space::XXL);
    page = page.push(tiles(data, a, b));
    let files_card = files(data, a, b, v.compare);
    let diff_card = diff_panel(a, b, v.compare);
    page = page.push(
        row![
            container(files_card).width(Length::FillPortion(2)),
            container(diff_card).width(Length::FillPortion(3)),
        ]
        .spacing(theme::space::XXL),
    );
    page = page.push(
        row![
            container(tests_card(v)).width(Length::FillPortion(1)),
            container(prompt_card(a, b, v)).width(Length::FillPortion(1)),
        ]
        .spacing(theme::space::XXL),
    );
    page = page.push(muted(
        "Counts are lines from git diff against the common commit, including uncommitted \
         changes; untracked files count as all added (up to 100 per worktree, 256 KB each); \
         binary files have no line count. More lines is not better or worse: this page \
         shows facts, not a verdict. Esc closes.",
    ));
    page.into()
}

fn header<'a>(
    data: &'a CompareData,
    a: &'a Side,
    b: &'a Side,
    choices: Vec<Choice>,
    v: &View<'a>,
) -> Element<'a, Message> {
    let picker = |label: &'a str, side: &'a Side, on: fn(Choice) -> Message| {
        column![
            card_title(label),
            pick_list(choices.clone(), Some(choice(&side.worktree)), on)
                .text_size(13)
                .padding([5, 10])
                .width(Length::Fixed(300.0)),
        ]
        .spacing(theme::space::XS)
    };
    let base = match &data.base {
        Ok(h) => format!("common commit {}", &h[..h.len().min(8)]),
        Err(e) => format!("no common commit: {e}"),
    };
    let age = v.now.saturating_duration_since(data.loaded_at).as_secs();
    let updated = if v.compare.busy {
        "updating…".to_owned()
    } else {
        format!("updated {age}s ago")
    };
    let actions = row![
        text(updated)
            .size(theme::size::TINY)
            .color(theme::text_muted()),
        action_button(
            "Swap A ⇄ B".to_owned(),
            Some(Message::Command(CommandId::CompareSwap)),
            theme::cyan(),
            false,
        ),
        action_button(
            "Refresh".to_owned(),
            Some(Message::Command(CommandId::GitRefresh)),
            theme::cyan(),
            false,
        ),
    ]
    .spacing(theme::space::M)
    .align_y(iced::Alignment::Center);
    card(
        column![
            row![
                text("⇄").size(20).color(theme::cyan()),
                text("Compare Worktrees").size(22).color(theme::text()),
                space::horizontal(),
                actions,
            ]
            .spacing(theme::space::L)
            .align_y(iced::Alignment::Center),
            row![
                picker("A", a, Message::ComparePickA),
                picker("B", b, Message::ComparePickB),
                column![
                    card_title("BASE"),
                    text(base)
                        .size(theme::size::SMALL)
                        .font(TERMINAL_FONT)
                        .color(theme::violet()),
                ]
                .spacing(theme::space::M),
            ]
            .spacing(20)
            .align_y(iced::Alignment::End),
        ]
        .spacing(theme::space::XL),
    )
}

fn tiles<'a>(data: &'a CompareData, a: &'a Side, b: &'a Side) -> Element<'a, Message> {
    let side_tile = |label: &'a str, s: &'a Side, accent: Color| match &s.files {
        Ok(f) => {
            let (add, del) = totals(f);
            tile(
                label,
                format!("{} file{}", f.len(), plural(f.len())),
                format!(
                    "+{add} −{del} lines · {}",
                    s.worktree.branch.as_deref().unwrap_or("detached")
                ),
                accent,
            )
        }
        Err(e) => tile(label, "—".to_owned(), e.clone(), theme::amber()),
    };
    let both = match (&a.files, &b.files) {
        (Ok(_), Ok(_)) => {
            let n = data.rows.iter().filter(|r| r.in_both()).count();
            tile(
                "CHANGED IN BOTH",
                n.to_string(),
                if n == 0 {
                    "no file changed on both sides".to_owned()
                } else {
                    "may need care when merging both".to_owned()
                },
                theme::amber(),
            )
        }
        _ => tile(
            "CHANGED IN BOTH",
            "—".to_owned(),
            "not available".to_owned(),
            theme::amber(),
        ),
    };
    row![
        side_tile("A · CHANGED", a, theme::cyan()),
        side_tile("B · CHANGED", b, theme::violet()),
        both,
    ]
    .spacing(theme::space::XL)
    .into()
}

fn count_label(f: Option<&ChangedFile>) -> (String, Color) {
    match f {
        None => ("—".to_owned(), theme::disabled()),
        Some(f) => match (f.added, f.deleted) {
            (Some(a), Some(_)) if f.untracked => (format!("new +{a}"), theme::text()),
            (Some(a), Some(d)) => (format!("+{a} −{d}"), theme::text()),
            _ if f.untracked => ("new".to_owned(), theme::text()),
            _ => ("binary".to_owned(), theme::text_muted()),
        },
    }
}

/// Rows drawn in the file list; the rest is counted.
const MAX_FILES_SHOWN: usize = 500;

fn files<'a>(
    data: &'a CompareData,
    a: &'a Side,
    b: &'a Side,
    c: &'a Compare,
) -> Element<'a, Message> {
    let mut list =
        column![card_title("FILES CHANGED SINCE THE COMMON COMMIT")].spacing(theme::space::S);
    let (fa, fb) = match (&a.files, &b.files) {
        (Ok(fa), Ok(fb)) => (fa.as_slice(), fb.as_slice()),
        (Err(e), _) => return card(list.push(muted_owned(format!("A: {e}")))),
        (_, Err(e)) => return card(list.push(muted_owned(format!("B: {e}")))),
    };
    let rows = &data.rows;
    if rows.is_empty() {
        return card(list.push(muted(
            "Neither worktree changed anything since the common commit.",
        )));
    }
    let max = rows
        .iter()
        .flat_map(|r| [r.a.map(|i| &fa[i]), r.b.map(|i| &fb[i])])
        .flatten()
        .map(|f| f.added.unwrap_or(0) + f.deleted.unwrap_or(0))
        .max()
        .unwrap_or(1)
        .max(1);
    list = list.push(
        row![
            container(muted("file")).width(Length::Fill),
            container(text("A").size(theme::size::CAPTION).color(theme::cyan())).width(110),
            container(text("B").size(theme::size::CAPTION).color(theme::violet())).width(110),
        ]
        .spacing(theme::space::M),
    );
    for r in rows.iter().take(MAX_FILES_SHOWN) {
        let (ra, rb) = (r.a.map(|i| &fa[i]), r.b.map(|i| &fb[i]));
        let selected = c.selected.as_deref() == Some(r.path.as_str());
        let cell = |f: Option<&ChangedFile>| -> Element<'a, Message> {
            let (label, color) = count_label(f);
            let bar: Element<'a, Message> = match f.and_then(|f| f.added.zip(f.deleted)) {
                Some((ad, de)) => diff_bar(ad, de, max),
                None => space::horizontal().width(0).into(),
            };
            column![
                text(label)
                    .size(theme::size::TINY)
                    .font(TERMINAL_FONT)
                    .color(color),
                container(bar).width(100).clip(true),
            ]
            .spacing(theme::space::XXS)
            .width(110)
            .into()
        };
        let mut name = row![
            text(r.path.clone())
                .size(theme::size::SMALL)
                .color(if selected {
                    theme::cyan()
                } else {
                    theme::text()
                })
                .wrapping(text::Wrapping::None),
        ]
        .spacing(theme::space::S)
        .align_y(iced::Alignment::Center);
        if r.in_both() {
            name = name.push(tag("both", theme::amber()));
        }
        let line = row![
            container(name).width(Length::Fill).clip(true),
            cell(ra),
            cell(rb),
        ]
        .spacing(theme::space::M)
        .align_y(iced::Alignment::Center);
        list = list.push(with_tip(
            mouse_area(
                container(line)
                    .padding([3, 6])
                    .width(Length::Fill)
                    .style(move |_| container::Style {
                        background: selected.then(|| theme::elevated().into()),
                        border: Border {
                            radius: theme::radius::S.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            )
            .on_press(Message::CompareSelect(r.path.clone()))
            .interaction(iced::mouse::Interaction::Pointer),
            format!("{}\nClick: this file in A beside B", r.path),
        ));
    }
    if rows.len() > MAX_FILES_SHOWN {
        list = list.push(muted_owned(format!(
            "… and {} more files (only the first {MAX_FILES_SHOWN} are listed)",
            rows.len() - MAX_FILES_SHOWN
        )));
    }
    card(list)
}

fn diff_panel<'a>(a: &'a Side, b: &'a Side, c: &'a Compare) -> Element<'a, Message> {
    let head = row![
        container(
            text(format!("A · {}", tail(&name_of(&a.worktree), 40)))
                .size(theme::size::TINY)
                .color(theme::cyan())
        )
        .width(Length::Fill),
        container(
            text(format!("B · {}", tail(&name_of(&b.worktree), 40)))
                .size(theme::size::TINY)
                .color(theme::violet())
        )
        .width(Length::Fill),
    ];
    let mut col = column![card_title("SIDE BY SIDE"), head].spacing(theme::space::M);
    let Some(d) = &c.diff else {
        return card(col.push(muted("Click a file to see it in A beside B.")));
    };
    col = col.push(
        text(d.path.clone())
            .size(theme::size::SMALL)
            .font(TERMINAL_FONT)
            .color(theme::text()),
    );
    let body: Element<'a, Message> = match &d.result {
        None => muted("Reading…"),
        Some(Err(e)) => muted_owned(format!("Not available: {e}")),
        Some(Ok((sbs, _))) if sbs.binary => muted("Binary file: the two versions differ."),
        Some(Ok((sbs, _))) if sbs.rows.is_empty() => muted("Identical in A and B."),
        Some(Ok((sbs, truncated))) => {
            let mut rows = column![].spacing(0);
            for r in sbs.rows.iter().take(MAX_ROWS_SHOWN) {
                rows = rows.push(diff_row(r));
            }
            if sbs.rows.len() > MAX_ROWS_SHOWN || *truncated {
                rows = rows.push(muted_owned(format!(
                    "… only the first {MAX_ROWS_SHOWN} rows / {} diff lines are shown",
                    forgeterm_git::MAX_DIFF_LINES
                )));
            }
            container(
                scrollable(rows)
                    .direction(scrollable::Direction::Vertical(
                        scrollable::Scrollbar::new().width(6).scroller_width(6),
                    ))
                    .height(Length::Fixed(420.0)),
            )
            .into()
        }
    };
    card(col.push(body))
}

fn diff_row(r: &Row) -> Element<'_, Message> {
    if r.kind == RowKind::Hunk {
        let t = r.left.as_ref().map_or("", |(_, t)| t.as_str());
        return container(
            text(t)
                .size(theme::size::TINY)
                .font(TERMINAL_FONT)
                .color(theme::violet()),
        )
        .padding([2, 4])
        .into();
    }
    // The sides' own colors, not red/green: neither side is "old" or "wrong".
    let (lc, rc) = match r.kind {
        RowKind::Changed => (Some(theme::cyan()), Some(theme::violet())),
        RowKind::Removed => (Some(theme::cyan()), None),
        RowKind::Added => (None, Some(theme::violet())),
        _ => (None, None),
    };
    row![half(r.left.as_ref(), lc), half(r.right.as_ref(), rc)]
        .spacing(theme::space::XS)
        .into()
}

fn half(cell: Option<&(u32, String)>, tint: Option<Color>) -> Element<'_, Message> {
    let (no, line) = match cell {
        Some((n, t)) => (n.to_string(), t.as_str()),
        None => (String::new(), ""),
    };
    container(
        row![
            container(
                text(no)
                    .size(theme::size::TINY)
                    .font(TERMINAL_FONT)
                    .color(theme::disabled())
            )
            .width(40)
            .align_x(iced::alignment::Horizontal::Right),
            text(line.replace('\t', "    "))
                .size(theme::size::SMALL)
                .font(TERMINAL_FONT)
                .color(theme::text())
                .wrapping(text::Wrapping::None),
        ]
        .spacing(theme::space::M),
    )
    .width(Length::Fill)
    .clip(true)
    .style(move |_| container::Style {
        background: tint.map(|c| Color { a: 0.13, ..c }.into()),
        ..Default::default()
    })
    .into()
}

fn tests_card<'a>(v: &View<'a>) -> Element<'a, Message> {
    let c = v.compare;
    let input = text_input("test command, e.g. cargo test", &c.test_command)
        .id(TEST_INPUT_ID)
        .on_input(Message::CompareTestCommand)
        .font(TERMINAL_FONT)
        .size(theme::size::BODY)
        .padding([6, 8]);
    let can = !c.test_command.trim().is_empty();
    let mut col = column![
        card_title("TESTS"),
        text(
            "Opens a tab with a terminal in A beside one in B and types this command in \
             both, without Enter: you start each run yourself."
        )
        .size(theme::size::SMALL)
        .color(theme::text_muted()),
        row![
            input,
            action_button(
                "Open test terminals".to_owned(),
                can.then_some(Message::Command(CommandId::CompareOpenTests)),
                theme::cyan(),
                true,
            ),
        ]
        .spacing(theme::space::M)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(theme::space::M);
    for (label, (id, state), color) in [
        ("A", &v.tests[0], theme::cyan()),
        ("B", &v.tests[1], theme::violet()),
    ] {
        let (msg, msg_color) = match state {
            TestState::None => ("no test terminal".to_owned(), theme::text_muted()),
            TestState::Inserting => (
                "waiting for the shell's prompt…".to_owned(),
                theme::text_muted(),
            ),
            TestState::Ended => ("the terminal has ended".to_owned(), theme::text_muted()),
            TestState::NothingYet => (
                "ready: press Enter in the terminal to run it".to_owned(),
                theme::text(),
            ),
            TestState::Running(cmd) => (
                format!("running {}", cmd.as_deref().unwrap_or("")),
                theme::amber(),
            ),
            TestState::Finished(cmd, Some(0)) => (
                format!(
                    "{} finished with exit code 0",
                    cmd.as_deref().unwrap_or("command")
                ),
                theme::mint(),
            ),
            TestState::Finished(cmd, Some(code)) => (
                format!(
                    "{} finished with exit code {code}",
                    cmd.as_deref().unwrap_or("command")
                ),
                theme::coral(),
            ),
            TestState::Finished(cmd, None) => (
                format!(
                    "{} finished; the shell did not report an exit code",
                    cmd.as_deref().unwrap_or("command")
                ),
                theme::text(),
            ),
            TestState::NotReported => (
                "exit code not available: this shell does not report commands (see the terminal)"
                    .to_owned(),
                theme::text_muted(),
            ),
        };
        let mut line = row![
            text(label).size(theme::size::SMALL).color(color),
            text(msg).size(theme::size::SMALL).color(msg_color),
            space::horizontal(),
        ]
        .spacing(theme::space::L)
        .align_y(iced::Alignment::Center);
        if let Some(id) = id {
            line = line.push(action_button(
                "Go to".to_owned(),
                Some(Message::Command(CommandId::GoToTerminal(*id))),
                theme::cyan(),
                false,
            ));
        }
        col = col.push(line);
    }
    col = col.push(muted(
        "The exit code comes from shell integration; it says whether the command failed, \
         not how good the code is.",
    ));
    card(col)
}

fn prompt_card<'a>(a: &'a Side, b: &'a Side, v: &View<'a>) -> Element<'a, Message> {
    let c = v.compare;
    let editor = text_editor(&c.prompt)
        .placeholder("The same instruction for both agents")
        .on_action(Message::ComparePromptEdit)
        .size(theme::size::SMALL)
        .padding([6, 8])
        .min_height(60.0)
        .max_height(160.0)
        .style(|_, status| {
            let focused = matches!(status, text_editor::Status::Focused { .. });
            text_editor::Style {
                background: theme::obsidian().into(),
                border: Border {
                    color: if focused {
                        theme::cyan()
                    } else {
                        theme::border()
                    },
                    width: 1.0,
                    radius: theme::radius::M.into(),
                },
                placeholder: theme::text_muted(),
                value: theme::text(),
                selection: theme::selection(),
            }
        });
    let has_text = !c.prompt.text().trim().is_empty();
    let any_agent = v.agents.iter().any(|l| !l.is_empty());
    let mut col = column![
        card_title("ONE PROMPT FOR BOTH AGENTS"),
        editor,
        row![
            space::horizontal(),
            action_button(
                "Insert into both (no Enter)".to_owned(),
                (has_text && any_agent).then_some(Message::Command(CommandId::CompareInsertPrompt)),
                theme::cyan(),
                true,
            ),
        ],
    ]
    .spacing(theme::space::M);
    for (label, side, agents, color) in [
        ("A", a, &v.agents[0], theme::cyan()),
        ("B", b, &v.agents[1], theme::violet()),
    ] {
        let mut line = row![text(label).size(theme::size::SMALL).color(color)]
            .spacing(theme::space::L)
            .align_y(iced::Alignment::Center);
        match agents.first() {
            Some(ag) => {
                line = line.push(agent_badge(ag.kind));
                line = line.push(
                    text(format!("{} · tab {}", ag.kind.name(), ag.tab + 1))
                        .size(theme::size::SMALL)
                        .color(theme::text()),
                );
            }
            None => {
                line = line.push(
                    text(format!(
                        "no agent of this window works in {}",
                        name_of(&side.worktree)
                    ))
                    .size(theme::size::SMALL)
                    .color(theme::text_muted()),
                );
            }
        }
        col = col.push(line);
    }
    col = col.push(muted(
        "Inserted like a paste, never followed by Enter: read it in each agent and \
         send it yourself.",
    ));
    card(col)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wt(path: &str, linked: bool) -> Worktree {
        Worktree {
            path: PathBuf::from(path),
            head: Some("abc1234".to_owned()),
            branch: Some(if linked { "agent/x" } else { "main" }.to_owned()),
            detached: false,
            bare: false,
            locked: None,
            prunable: None,
        }
    }

    #[test]
    fn picks_two_agents_first_then_main_and_one() {
        let main = wt("C:/w/shop", false);
        let a = wt("C:/w/shop-worktrees/a", true);
        let b = wt("C:/w/shop-worktrees/b", true);
        assert_eq!(pick_sides(std::slice::from_ref(&main), None, None), None);
        assert_eq!(
            pick_sides(&[main.clone(), a.clone()], None, None),
            Some((0, 1))
        );
        let three = [main.clone(), a.clone(), b.clone()];
        assert_eq!(pick_sides(&three, None, None), Some((1, 2)));
        // Chosen sides win; the same worktree twice is never compared.
        assert_eq!(
            pick_sides(&three, Some(Path::new("C:/w/shop")), None),
            Some((0, 2))
        );
        assert_eq!(
            pick_sides(
                &three,
                Some(Path::new("C:/w/shop-worktrees/b")),
                Some(Path::new("C:/w/shop-worktrees/b"))
            ),
            Some((2, 1))
        );
        // A chosen worktree that is gone falls back to the default.
        assert_eq!(
            pick_sides(&three, Some(Path::new("C:/gone")), None),
            Some((1, 2))
        );
        // Bare and prunable ones are skipped.
        let mut gone = b.clone();
        gone.prunable = Some(String::new());
        assert_eq!(pick_sides(&[main, a, gone], None, None), Some((0, 1)));
    }

    #[test]
    fn rows_merge_both_sides() {
        let f = |p: &str, a: u32| ChangedFile {
            path: p.to_owned(),
            added: Some(a),
            deleted: Some(0),
            untracked: false,
        };
        let a = vec![f("src/a.rs", 1), f("x.txt", 2)];
        let b = vec![f("src/a.rs", 3), f("b.rs", 4)];
        let rows = file_rows(&a, &b);
        let paths: Vec<&str> = rows.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, ["b.rs", "src/a.rs", "x.txt"]);
        assert!(rows[1].in_both());
        assert_eq!((rows[1].a, rows[1].b), (Some(0), Some(0)));
        assert_eq!((rows[0].a, rows[0].b), (None, Some(1)));
        assert_eq!((rows[2].a, rows[2].b), (Some(1), None));
        assert_eq!(totals(&a), (3, 0));
    }

    #[test]
    fn inserts_at_the_prompt_or_after_quiet_output() {
        assert!(ready_for_insert(true, None));
        assert!(!ready_for_insert(false, None));
        assert!(!ready_for_insert(false, Some(Duration::from_millis(200))));
        assert!(ready_for_insert(false, Some(Duration::from_secs(2))));
    }
}
