//! The code viewer.

use super::*;

impl App {
    pub(super) fn viewer_msg(&mut self, msg: ViewerMsg) -> Task<Message> {
        let v = &mut self.viewer;
        match msg {
            ViewerMsg::Open(path) => {
                v.visible = true;
                v.error = None;
                if let Some(i) = v.index_of(&path) {
                    v.active = i;
                    v.focused = true;
                    self.explorer_focused = false;
                    if let Some((p, line)) = v.pending_line.take()
                        && p == path
                    {
                        v.tabs[i].go_to_line(line);
                    }
                    if v.find_open {
                        v.refresh_matches();
                    }
                    return Task::none();
                }
                return load_document(path);
            }
            ViewerMsg::Loaded(path, result) => match result {
                Err(e) => {
                    // A later, unrelated open of this file must not jump to the line.
                    v.pending_line.take_if(|(p, _)| *p == path);
                    if let Some(i) = v.index_of(&path) {
                        v.tabs[i].reloading = false;
                    } else {
                        v.error = Some(e);
                    }
                }
                Ok(doc) => {
                    let index = match v.index_of(&path) {
                        Some(i) => {
                            v.tabs[i].replace(*doc);
                            i
                        }
                        None => {
                            v.tabs.push(ViewerTab::new(*doc));
                            v.active = v.tabs.len() - 1;
                            v.focused = true;
                            self.explorer_focused = false;
                            v.tabs.len() - 1
                        }
                    };
                    if index == v.active && v.find_open {
                        v.refresh_matches();
                    }
                    let pending = v.pending_line.take_if(|(p, _)| *p == path);
                    let tab = &mut v.tabs[index];
                    if let Some((_, line)) = pending {
                        tab.go_to_line(line);
                    }
                    if tab.doc.highlightable() && tab.doc.syntax != "Plain Text" {
                        tab.highlighting = true;
                        let doc = tab.doc.clone();
                        return background(
                            move || {
                                let spans = forgeterm_viewer::highlight(&doc);
                                (doc.path, doc.modified, spans)
                            },
                            |(path, modified, spans)| {
                                Message::Viewer(ViewerMsg::Highlighted(path, modified, spans))
                            },
                        );
                    }
                }
            },
            ViewerMsg::Highlighted(path, modified, spans) => {
                // Only for the version that is still shown.
                if let Some(t) = v
                    .tabs
                    .iter_mut()
                    .find(|t| t.doc.path == path && t.doc.modified == modified)
                {
                    t.spans = spans;
                    t.highlighting = false;
                    t.cache.clear();
                }
            }
            ViewerMsg::Scroll(lines) => {
                if let Some(t) = v.tab_mut() {
                    t.scroll_by(lines);
                }
            }
            ViewerMsg::ScrollFraction(f) => {
                v.focused = true;
                self.explorer_focused = false;
                if let Some(t) = v.tab_mut() {
                    t.scroll_to_fraction(f);
                }
            }
            ViewerMsg::CenterOn(line) => {
                v.focused = true;
                self.explorer_focused = false;
                if let Some(t) = v.tab_mut() {
                    t.center_on(line);
                }
            }
            ViewerMsg::Rows(rows) => {
                if let Some(t) = v.tab_mut() {
                    t.rows = rows.max(1);
                    t.top = t.top.min(t.doc.line_count().saturating_sub(t.rows));
                    t.cache.clear();
                }
            }
            ViewerMsg::SelectStart(line, extend) => {
                v.focused = true;
                self.explorer_focused = false;
                if let Some(t) = v.tab_mut() {
                    t.selection = match (extend, t.selection) {
                        (true, Some((anchor, _))) => Some((anchor, line)),
                        _ => Some((line, line)),
                    };
                    t.cache.clear();
                }
            }
            ViewerMsg::SelectTo(line) => {
                if let Some(t) = v.tab_mut() {
                    let line = line.min(t.doc.line_count().saturating_sub(1));
                    if let Some((anchor, _)) = t.selection {
                        t.selection = Some((anchor, line));
                    }
                    t.reveal(line);
                }
            }
            ViewerMsg::SelectTab(i) => {
                if i < v.tabs.len() {
                    v.active = i;
                    v.focused = true;
                    self.explorer_focused = false;
                    if v.find_open {
                        v.refresh_matches();
                    }
                }
            }
            ViewerMsg::CloseTab(i) => {
                if i < v.tabs.len() {
                    v.tabs.remove(i);
                    if v.active >= v.tabs.len() {
                        v.active = v.tabs.len().saturating_sub(1);
                    } else if i < v.active {
                        v.active -= 1;
                    }
                    if v.tabs.is_empty() {
                        v.focused = false;
                    }
                    v.refresh_matches();
                }
            }
            ViewerMsg::FindQuery(q) => {
                v.query = q;
                v.refresh_matches();
            }
            ViewerMsg::FindNext => v.step_match(true),
            ViewerMsg::FindPrev => v.step_match(false),
            ViewerMsg::OpenFind => {
                v.visible = true;
                v.find_open = true;
                v.refresh_matches();
                return iced::widget::operation::focus(viewer_view::FIND_INPUT_ID);
            }
            ViewerMsg::CloseFind => {
                v.find_open = false;
                v.focused = !v.tabs.is_empty();
                if let Some(t) = v.tab_mut() {
                    t.cache.clear();
                }
            }
            ViewerMsg::CopySelection => {
                if let Some(t) = v.tab()
                    && let Some((a, b)) = t.selected_range()
                {
                    let text = forgeterm_viewer::copy_lines(&t.doc, a, b);
                    let text = if cfg!(windows) {
                        text.replace('\n', "\r\n")
                    } else {
                        text
                    };
                    return clipboard::write(text);
                }
            }
            ViewerMsg::CopyPath => {
                if let Some(t) = v.tab() {
                    return clipboard::write(t.doc.path.display().to_string());
                }
            }
            ViewerMsg::InsertPath => {
                if let Some(path) = v.tab().map(|t| t.doc.path.clone()) {
                    self.insert_paths(&[path]);
                }
            }
            ViewerMsg::Reload => {
                if let Some(t) = v.tab_mut() {
                    t.reloading = true;
                    return load_document(t.doc.path.clone());
                }
            }
            ViewerMsg::ClosePanel => {
                v.visible = false;
                v.focused = false;
            }
            ViewerMsg::Escape => {
                let had_selection = v.tab().is_some_and(|t| t.selection.is_some());
                if let Some(t) = v.tab_mut().filter(|_| had_selection) {
                    t.selection = None;
                    t.cache.clear();
                } else {
                    v.focused = false;
                }
            }
            ViewerMsg::SelectAll => {
                if let Some(t) = v.tab_mut() {
                    t.selection = Some((0, t.doc.line_count().saturating_sub(1)));
                    t.cache.clear();
                }
            }
        }
        Task::none()
    }
}
