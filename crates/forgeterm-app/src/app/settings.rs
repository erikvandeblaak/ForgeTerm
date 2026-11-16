//! Applying settings.toml (only what changed).

use super::*;

impl App {
    /// (Re)loads settings.toml and applies what changed. At startup everything is
    /// applied; on reload only settings whose value in the file changed, so runtime
    /// toggles (zoom, explorer) are not reset by unrelated edits.
    pub(super) fn load_settings(&mut self, startup: bool) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        self.settings_mtime = file_mtime(&path);
        let loaded = forgeterm_settings::load(&path);
        // A half-edited file with a syntax error must not reset a running app to
        // defaults: keep the current settings and only report the error.
        if loaded.unreadable && !startup {
            self.settings_problems = loaded
                .problems
                .into_iter()
                .map(|p| format!("{p} (current settings kept)"))
                .collect();
            return;
        }
        let mut problems = loaded.problems;
        let new = loaded.settings;
        let old = std::mem::replace(&mut self.settings, new.clone());

        if let Some(name) = &new.terminal.default_shell
            && !self
                .shells
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(name))
        {
            let available: Vec<&str> = self.shells.iter().map(|s| s.name.as_str()).collect();
            problems.push(format!(
                "terminal.default_shell = \"{name}\" is not installed; available: {}",
                available.join(", ")
            ));
        }
        // Only a local folder is checked: `is_dir` on a network path can block the
        // window for as long as the share takes to answer.
        self.start_dir = match &new.terminal.start_directory {
            Some(dir)
                if !dir.to_str().is_some_and(forgeterm_terminal::is_local_path)
                    || !forgeterm_fs::resolves_locally(dir) =>
            {
                problems.push(format!(
                    "terminal.start_directory = \"{}\" is not a local folder; using {}",
                    dir.display(),
                    launch_dir().display()
                ));
                launch_dir()
            }
            Some(dir) if dir.is_dir() => dir.clone(),
            Some(dir) => {
                problems.push(format!(
                    "terminal.start_directory = \"{}\" is not a folder; using {}",
                    dir.display(),
                    launch_dir().display()
                ));
                launch_dir()
            }
            None => launch_dir(),
        };

        if startup || old.appearance.theme != new.appearance.theme {
            self.apply_theme(new.appearance.theme);
        }
        let (t_old, t_new) = (&old.terminal, &new.terminal);
        if startup || t_old.font_size != t_new.font_size {
            self.set_font_size(t_new.font_size);
        }
        let (e_old, e_new) = (&old.explorer, &new.explorer);
        if startup || e_old.visible != e_new.visible {
            // `explorer.visible = true` opens it (closing any other side panel);
            // false closes it only if it is the one shown.
            if e_new.visible {
                self.side = Some(SidePanel::Explorer);
            } else if self.panel_open(SidePanel::Explorer) {
                self.side = None;
            }
        }
        if startup || e_old.follow_terminal != e_new.follow_terminal {
            self.explorer_follow = e_new.follow_terminal;
            self.explorer_synced = None;
        }
        if startup || e_old.show_hidden != e_new.show_hidden {
            self.explorer.set_show_hidden(e_new.show_hidden);
        }
        if startup && self.explorer.root() != self.start_dir {
            self.explorer.set_root(self.start_dir.clone());
        }
        // Shortcuts: defaults plus [keys]; rebuilt on every load (cheap).
        let (keymap, key_problems) = Keymap::with_overrides(&new.keys);
        self.keymap = keymap;
        problems.extend(key_problems);

        for p in &problems {
            tracing::warn!("settings: {p}");
        }
        self.settings_problems = problems;
    }

    /// Makes `choice` the active theme and redraws everything that caches colors:
    /// terminal snapshots carry resolved colors, canvases keep their geometry.
    pub(super) fn apply_theme(&mut self, choice: ThemeChoice) {
        theme::set(choice);
        self.palette = theme::terminal_palette();
        for t in self.terminals.values_mut() {
            if let Some(session) = t.session() {
                session.set_palette(&self.palette);
            }
            t.refresh(&self.palette);
            t.cache.clear();
            t.preview_cache.clear();
        }
        for tab in &self.viewer.tabs {
            tab.cache.clear();
        }
    }

    pub(super) fn settings_changed_on_disk(&self) -> bool {
        self.settings_path
            .as_deref()
            .is_some_and(|p| file_mtime(p) != self.settings_mtime)
    }
}
