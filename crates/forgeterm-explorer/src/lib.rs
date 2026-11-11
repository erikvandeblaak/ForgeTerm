//! File explorer model: reads directories and keeps an expandable tree.
//!
//! UI-agnostic and synchronous; the GUI runs [`list_dir`] off the UI thread and feeds
//! results back with [`Tree::set_listing`]. Nothing here modifies the file system.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// At most this many entries of one directory are shown; the rest is reported as
/// a count instead of silently dropped.
pub const MAX_ENTRIES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub hidden: bool,
}

/// Contents of one directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// Entries beyond [`MAX_ENTRIES`] that are not included.
    pub truncated: usize,
    /// The folder's modification time when it was read: it changes when an entry
    /// is added, removed or renamed, which is how [`changed_dirs`] finds work.
    pub modified: Option<SystemTime>,
}

/// Reads a directory: folders first, then files, each case-insensitively by name.
/// Entries whose metadata cannot be read are still listed (as files), so nothing
/// disappears silently.
pub fn list_dir(path: &Path) -> Result<Listing, String> {
    // Read before the entries: a change during the listing then shows up as a
    // newer time on the next check instead of being missed.
    let modified = fs::metadata(path).and_then(|m| m.modified()).ok();
    let read = fs::read_dir(path).map_err(|e| e.to_string())?;
    let mut entries: Vec<Entry> = read
        .filter_map(Result::ok)
        .map(|dirent| {
            let path = dirent.path();
            let name = dirent.file_name().to_string_lossy().into_owned();
            // `fs::metadata` follows symlinks/junctions, so a link to a folder is a folder.
            let metadata = fs::metadata(&path).or_else(|_| dirent.metadata()).ok();
            let is_dir = metadata.as_ref().is_some_and(fs::Metadata::is_dir);
            let hidden = is_hidden(&name, metadata.as_ref());
            Entry {
                name,
                path,
                is_dir,
                hidden,
            }
        })
        .collect();
    entries.sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase(), e.name.clone()));
    let truncated = entries.len().saturating_sub(MAX_ENTRIES);
    entries.truncate(MAX_ENTRIES);
    Ok(Listing {
        entries,
        truncated,
        modified,
    })
}

/// The folders among `shown` (path, time of the listing on screen) whose
/// modification time differs now: they need a fresh [`list_dir`]. A folder that
/// cannot be read anymore counts as changed, so its error becomes visible.
pub fn changed_dirs(shown: &[(PathBuf, Option<SystemTime>)]) -> Vec<PathBuf> {
    shown
        .iter()
        .filter(|(dir, seen)| fs::metadata(dir).and_then(|m| m.modified()).ok() != *seen)
        .map(|(dir, _)| dir.clone())
        .collect()
}

/// The folder typed into "Go to Folder": `~` / `~/x` from `home`, a relative
/// path against `current`. Only the text is interpreted; the caller checks that
/// it exists (off the UI thread) and that it is local.
pub fn folder_from_input(
    input: &str,
    current: &Path,
    home: Option<&Path>,
) -> Result<PathBuf, String> {
    let input = input.trim().trim_matches('"');
    if input.is_empty() {
        return Err("Type a folder path.".into());
    }
    let path = if input == "~" {
        home.ok_or("Home folder unknown.")?.to_path_buf()
    } else if let Some(rest) = input
        .strip_prefix("~/")
        .or_else(|| input.strip_prefix("~\\"))
    {
        home.ok_or("Home folder unknown.")?.join(rest)
    } else {
        current.join(input)
    };
    Ok(path)
}

/// Keyboard navigation in the tree (see [`Tree::navigate`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    /// Collapse the folder, or go to the parent folder's row.
    Left,
    /// Expand the folder, or go to its first child.
    Right,
    /// Enter: open/close a folder, open a file.
    Activate,
}

/// What the app must do after [`Tree::navigate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavOutcome {
    None,
    /// A folder was expanded and must be loaded.
    Load(PathBuf),
    /// Enter on a file.
    Open(PathBuf),
}

/// Dot-files everywhere; additionally the hidden attribute on Windows.
fn is_hidden(name: &str, metadata: Option<&fs::Metadata>) -> bool {
    if name.starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if metadata.is_some_and(|m| m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0) {
            return true;
        }
    }
    #[cfg(not(windows))]
    let _ = metadata;
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Loading,
    Ready(Listing),
    Failed(String),
}

/// One visible line of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row<'a> {
    Entry {
        depth: usize,
        entry: &'a Entry,
        expanded: bool,
    },
    /// Loading / error / empty / truncation notice under a folder.
    Note { depth: usize, text: String },
}

#[derive(Debug, Clone)]
pub struct Tree {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    listings: HashMap<PathBuf, State>,
    show_hidden: bool,
    selected: Option<PathBuf>,
}

impl Tree {
    /// A tree rooted at `root`. The caller must load the root (see [`Tree::set_root`]).
    pub fn new(root: PathBuf) -> Self {
        let mut tree = Self {
            root: PathBuf::new(),
            expanded: HashSet::new(),
            listings: HashMap::new(),
            show_hidden: false,
            selected: None,
        };
        tree.set_root(root);
        tree
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Re-roots the tree. Returns the directory to load.
    pub fn set_root(&mut self, root: PathBuf) -> PathBuf {
        self.root = root.clone();
        self.expanded.clear();
        self.listings.clear();
        self.selected = None;
        self.listings.insert(root.clone(), State::Loading);
        root
    }

    pub fn parent(&self) -> Option<PathBuf> {
        self.root.parent().map(Path::to_path_buf)
    }

    /// Stores the result of loading `path`. Results for directories that are no
    /// longer shown (collapsed or re-rooted meanwhile) are ignored.
    pub fn set_listing(&mut self, path: PathBuf, result: Result<Listing, String>) {
        if path != self.root && !self.expanded.contains(&path) {
            return;
        }
        let state = match result {
            Ok(listing) => State::Ready(listing),
            Err(e) => State::Failed(e),
        };
        self.listings.insert(path, state);
    }

    /// Expands or collapses a folder. Returns the folder to load, if it has no
    /// listing yet.
    pub fn toggle(&mut self, path: &Path) -> Option<PathBuf> {
        if self.expanded.remove(path) {
            // Drop cached listings below so re-expanding shows fresh contents.
            self.listings.retain(|p, _| !p.starts_with(path));
            self.expanded.retain(|p| !p.starts_with(path));
            return None;
        }
        self.expanded.insert(path.to_path_buf());
        if self.listings.contains_key(path) {
            return None;
        }
        self.listings.insert(path.to_path_buf(), State::Loading);
        Some(path.to_path_buf())
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    /// Directories to reload for a refresh (root and every expanded folder). Old
    /// contents stay visible until the new listing arrives.
    pub fn refresh(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::iter::once(self.root.clone())
            .chain(self.expanded.iter().cloned())
            .collect();
        dirs.sort();
        dirs
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    pub fn set_show_hidden(&mut self, show: bool) {
        self.show_hidden = show;
        if !show
            && let Some(sel) = &self.selected
            && !self
                .rows()
                .iter()
                .any(|r| row_path(r) == Some(sel.as_path()))
        {
            self.selected = None;
        }
    }

    pub fn select(&mut self, path: Option<PathBuf>) {
        self.selected = path;
    }

    pub fn selected(&self) -> Option<&Entry> {
        let sel = self.selected.as_ref()?;
        self.listings.values().find_map(|state| match state {
            State::Ready(l) => l.entries.iter().find(|e| &e.path == sel),
            _ => None,
        })
    }

    /// Folders on screen with the modification time of their listing (for
    /// [`changed_dirs`]). Folders still loading or failed are left out.
    pub fn shown_dirs(&self) -> Vec<(PathBuf, Option<SystemTime>)> {
        self.refresh()
            .into_iter()
            .filter_map(|dir| match self.listings.get(&dir) {
                Some(State::Ready(l)) => Some((dir, l.modified)),
                _ => None,
            })
            .collect()
    }

    /// Index of the selected entry in [`Tree::rows`] (for scrolling it into view).
    pub fn selected_row(&self) -> Option<usize> {
        let sel = self.selected.as_deref()?;
        self.rows().iter().position(|r| row_path(r) == Some(sel))
    }

    /// Moves the selection or opens/closes folders. `page` = rows per page.
    pub fn navigate(&mut self, nav: Nav, page: usize) -> NavOutcome {
        // (path, is_dir, expanded, depth) of every entry row, in order.
        let entries: Vec<(PathBuf, bool, bool, usize)> = self
            .rows()
            .iter()
            .filter_map(|r| match r {
                Row::Entry {
                    depth,
                    entry,
                    expanded,
                } => Some((entry.path.clone(), entry.is_dir, *expanded, *depth)),
                Row::Note { .. } => None,
            })
            .collect();
        if entries.is_empty() {
            return NavOutcome::None;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|sel| entries.iter().position(|e| &e.0 == sel));
        let Some(i) = current else {
            // Nothing selected yet: any key starts at the top (End at the bottom).
            let start = if nav == Nav::End {
                entries.len() - 1
            } else {
                0
            };
            self.selected = Some(entries[start].0.clone());
            return NavOutcome::None;
        };
        let last = entries.len() - 1;
        let page = page.max(1);
        let (path, is_dir, expanded, depth) = entries[i].clone();
        let target = match nav {
            Nav::Up => i.saturating_sub(1),
            Nav::Down => (i + 1).min(last),
            Nav::PageUp => i.saturating_sub(page),
            Nav::PageDown => (i + page).min(last),
            Nav::Home => 0,
            Nav::End => last,
            Nav::Left if is_dir && expanded => {
                self.toggle(&path);
                return NavOutcome::None;
            }
            Nav::Left => match entries[..i].iter().rposition(|e| e.3 + 1 == depth) {
                Some(parent) => parent,
                None => return NavOutcome::None,
            },
            Nav::Right if is_dir && !expanded => {
                return self
                    .toggle(&path)
                    .map_or(NavOutcome::None, NavOutcome::Load);
            }
            Nav::Right => match entries.get(i + 1) {
                Some(child) if is_dir && child.3 == depth + 1 => i + 1,
                _ => return NavOutcome::None,
            },
            Nav::Activate if is_dir => {
                return self
                    .toggle(&path)
                    .map_or(NavOutcome::None, NavOutcome::Load);
            }
            Nav::Activate => return NavOutcome::Open(path),
        };
        self.selected = Some(entries[target].0.clone());
        NavOutcome::None
    }

    /// The visible rows, depth-first.
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut rows = Vec::new();
        self.push_rows(&self.root, 0, &mut rows);
        rows
    }

    fn push_rows<'a>(&'a self, dir: &Path, depth: usize, rows: &mut Vec<Row<'a>>) {
        let note = |text: String| Row::Note { depth, text };
        match self.listings.get(dir) {
            None | Some(State::Loading) => rows.push(note("Loading…".into())),
            Some(State::Failed(e)) => rows.push(note(format!("Cannot read: {e}"))),
            Some(State::Ready(listing)) => {
                let mut shown = 0;
                for entry in &listing.entries {
                    if entry.hidden && !self.show_hidden {
                        continue;
                    }
                    shown += 1;
                    let expanded = entry.is_dir && self.expanded.contains(&entry.path);
                    rows.push(Row::Entry {
                        depth,
                        entry,
                        expanded,
                    });
                    if expanded {
                        self.push_rows(&entry.path, depth + 1, rows);
                    }
                }
                if listing.truncated > 0 {
                    rows.push(note(format!("… {} more not shown", listing.truncated)));
                } else if shown == 0 {
                    rows.push(note(if listing.entries.is_empty() {
                        "Empty folder".into()
                    } else {
                        "Only hidden items".into()
                    }));
                }
            }
        }
    }
}

fn row_path<'a>(row: &Row<'a>) -> Option<&'a Path> {
    match row {
        Row::Entry { entry, .. } => Some(&entry.path),
        Row::Note { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("forgeterm-explorer-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(rows: &[Row<'_>]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Entry { depth, entry, .. } => format!("{}{}", "  ".repeat(*depth), entry.name),
                Row::Note { depth, text } => format!("{}({text})", "  ".repeat(*depth)),
            })
            .collect()
    }

    #[test]
    fn listing_sorts_folders_first_case_insensitive() {
        let t = TempDir::new("sort");
        for f in ["b.txt", "A.txt", "c.md"] {
            fs::write(t.0.join(f), f).unwrap();
        }
        for d in ["zeta", "Alpha"] {
            fs::create_dir(t.0.join(d)).unwrap();
        }
        let l = list_dir(&t.0).unwrap();
        let got: Vec<_> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(got, ["Alpha", "zeta", "A.txt", "b.txt", "c.md"]);
        assert!(l.entries[0].is_dir && !l.entries[2].is_dir);
        assert_eq!(l.truncated, 0);
    }

    #[test]
    fn missing_directory_is_an_error_not_a_panic() {
        assert!(list_dir(Path::new("/definitely/not/here/forgeterm")).is_err());
    }

    #[test]
    fn dotfiles_are_hidden() {
        let t = TempDir::new("hidden");
        fs::write(t.0.join(".env"), "").unwrap();
        fs::write(t.0.join("visible"), "").unwrap();
        let l = list_dir(&t.0).unwrap();
        let env = l.entries.iter().find(|e| e.name == ".env").unwrap();
        assert!(env.hidden);
        assert!(
            !l.entries
                .iter()
                .find(|e| e.name == "visible")
                .unwrap()
                .hidden
        );
    }

    #[test]
    fn tree_expands_lazily_and_renders_depth_first() {
        let t = TempDir::new("tree");
        fs::create_dir_all(t.0.join("src/deep")).unwrap();
        fs::write(t.0.join("src/main.rs"), "").unwrap();
        fs::write(t.0.join("README.md"), "").unwrap();
        fs::write(t.0.join(".git"), "").unwrap();

        let mut tree = Tree::new(t.0.clone());
        assert_eq!(names(&tree.rows()), ["(Loading…)"]);
        tree.set_listing(t.0.clone(), list_dir(&t.0));
        assert_eq!(names(&tree.rows()), ["src", "README.md"]);

        let src = t.0.join("src");
        let load = tree.toggle(&src);
        assert_eq!(load.as_deref(), Some(src.as_path()));
        assert_eq!(names(&tree.rows()), ["src", "  (Loading…)", "README.md"]);
        tree.set_listing(src.clone(), list_dir(&src));
        assert_eq!(
            names(&tree.rows()),
            ["src", "  deep", "  main.rs", "README.md"]
        );

        let deep = src.join("deep");
        tree.toggle(&deep);
        tree.set_listing(deep.clone(), list_dir(&deep));
        assert_eq!(
            names(&tree.rows()),
            [
                "src",
                "  deep",
                "    (Empty folder)",
                "  main.rs",
                "README.md"
            ]
        );

        // Collapsing a parent also forgets its expanded children.
        assert_eq!(tree.toggle(&src), None);
        assert_eq!(names(&tree.rows()), ["src", "README.md"]);
        assert!(!tree.is_expanded(&deep));

        tree.set_show_hidden(true);
        assert_eq!(names(&tree.rows()), ["src", ".git", "README.md"]);
    }

    #[test]
    fn stale_results_are_ignored() {
        let t = TempDir::new("stale");
        fs::create_dir(t.0.join("a")).unwrap();
        let mut tree = Tree::new(t.0.clone());
        tree.set_listing(t.0.clone(), list_dir(&t.0));
        let a = t.0.join("a");
        tree.toggle(&a);
        tree.toggle(&a); // collapsed before the listing arrived
        tree.set_listing(a.clone(), list_dir(&a));
        assert_eq!(names(&tree.rows()), ["a"]);

        // A listing for the old root after re-rooting is ignored too.
        tree.set_root(a.clone());
        tree.set_listing(t.0.clone(), list_dir(&t.0));
        assert_eq!(names(&tree.rows()), ["(Loading…)"]);
    }

    /// Builds a tree for `dir` with every listing loaded synchronously.
    fn loaded(dir: &Path) -> Tree {
        let mut tree = Tree::new(dir.to_path_buf());
        tree.set_listing(dir.to_path_buf(), list_dir(dir));
        tree
    }

    fn selected_name(tree: &Tree) -> String {
        tree.selected().map(|e| e.name.clone()).unwrap_or_default()
    }

    #[test]
    fn keyboard_navigation() {
        let t = TempDir::new("nav");
        fs::create_dir_all(t.0.join("a/inner")).unwrap();
        fs::write(t.0.join("a/x.rs"), "").unwrap();
        fs::create_dir(t.0.join("b")).unwrap();
        fs::write(t.0.join("c.txt"), "").unwrap();
        let mut tree = loaded(&t.0);

        // First key selects the top row; End the last.
        assert_eq!(tree.navigate(Nav::Down, 10), NavOutcome::None);
        assert_eq!(selected_name(&tree), "a");
        tree.navigate(Nav::End, 10);
        assert_eq!(selected_name(&tree), "c.txt");
        assert_eq!(tree.selected_row(), Some(2));
        // Enter on a file opens it.
        assert_eq!(
            tree.navigate(Nav::Activate, 10),
            NavOutcome::Open(t.0.join("c.txt"))
        );
        tree.navigate(Nav::Home, 10);
        assert_eq!(selected_name(&tree), "a");

        // Right expands (load), Right again goes to the first child.
        let a = t.0.join("a");
        assert_eq!(tree.navigate(Nav::Right, 10), NavOutcome::Load(a.clone()));
        assert_eq!(tree.navigate(Nav::Right, 10), NavOutcome::None);
        assert_eq!(selected_name(&tree), "a", "children not loaded yet");
        tree.set_listing(a.clone(), list_dir(&a));
        tree.navigate(Nav::Right, 10);
        assert_eq!(selected_name(&tree), "inner");
        tree.navigate(Nav::Down, 10);
        assert_eq!(selected_name(&tree), "x.rs");
        // Left on a file goes to the parent folder's row; Left again collapses.
        tree.navigate(Nav::Left, 10);
        assert_eq!(selected_name(&tree), "a");
        tree.navigate(Nav::Left, 10);
        assert!(!tree.is_expanded(&a));
        // Left at the top level has nowhere to go.
        assert_eq!(tree.navigate(Nav::Left, 10), NavOutcome::None);
        assert_eq!(selected_name(&tree), "a");
        // Pages are clamped; Up at the top stays.
        tree.navigate(Nav::PageDown, 10);
        assert_eq!(selected_name(&tree), "c.txt");
        tree.navigate(Nav::PageUp, 1);
        assert_eq!(selected_name(&tree), "b");
        tree.navigate(Nav::Home, 1);
        tree.navigate(Nav::Up, 1);
        assert_eq!(selected_name(&tree), "a");
        // Enter on a folder toggles it (already listed once: re-read).
        assert_eq!(tree.navigate(Nav::Activate, 1), NavOutcome::Load(a.clone()));
        assert!(tree.is_expanded(&a));
    }

    #[test]
    fn changes_are_detected_by_folder_time() {
        let t = TempDir::new("watch");
        fs::write(t.0.join("one.txt"), "").unwrap();
        let tree = loaded(&t.0);
        let shown = tree.shown_dirs();
        assert_eq!(shown.len(), 1);
        assert!(shown[0].1.is_some());
        assert!(changed_dirs(&shown).is_empty());
        // Coarse file-system clocks: wait until a new entry gives a newer time.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut n = 0;
        while changed_dirs(&shown).is_empty() && std::time::Instant::now() < deadline {
            n += 1;
            fs::write(t.0.join(format!("new{n}.txt")), "").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(changed_dirs(&shown), vec![t.0.clone()]);
        // A folder that disappeared counts as changed (its error gets shown).
        let gone = vec![(t.0.join("gone"), shown[0].1)];
        assert_eq!(changed_dirs(&gone), vec![t.0.join("gone")]);
    }

    #[test]
    fn folder_input() {
        let cur = Path::new("/work/proj");
        let home = Path::new("/home/me");
        assert_eq!(
            folder_from_input("src", cur, Some(home)).unwrap(),
            cur.join("src")
        );
        assert_eq!(folder_from_input(" ~ ", cur, Some(home)).unwrap(), home);
        assert_eq!(
            folder_from_input("~/code", cur, Some(home)).unwrap(),
            home.join("code")
        );
        let abs = if cfg!(windows) { r"C:\Users" } else { "/usr" };
        assert_eq!(
            folder_from_input(&format!("\"{abs}\""), cur, None).unwrap(),
            PathBuf::from(abs)
        );
        assert!(folder_from_input("  ", cur, None).is_err());
        assert!(folder_from_input("~", cur, None).is_err());
    }

    #[test]
    fn errors_and_selection() {
        let t = TempDir::new("sel");
        fs::write(t.0.join("f.txt"), "").unwrap();
        let mut tree = Tree::new(t.0.clone());
        tree.set_listing(t.0.clone(), Err("Access is denied.".into()));
        assert_eq!(names(&tree.rows()), ["(Cannot read: Access is denied.)"]);

        tree.set_listing(t.0.clone(), list_dir(&t.0));
        tree.select(Some(t.0.join("f.txt")));
        assert_eq!(tree.selected().map(|e| e.name.as_str()), Some("f.txt"));
        assert_eq!(tree.refresh(), vec![t.0.clone()]);
        assert_eq!(tree.parent(), t.0.parent().map(Path::to_path_buf));
    }
}
