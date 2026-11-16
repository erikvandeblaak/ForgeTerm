//! Docker containers: list, shell and logs in a terminal, start and stop. Only on the
//! user's command; ForgeTerm never removes containers, images or volumes.

use super::*;

/// A list younger than this is not read again when the palette opens.
const FRESH: Duration = Duration::from_secs(3);

impl App {
    pub(super) fn docker_container(&self, key: u64) -> Option<&forgeterm_docker::Container> {
        self.docker.containers.iter().find(|c| c.key() == key)
    }

    /// Reads `docker ps -a` in the background. `explicit`: the user asked, so a
    /// failure (no engine) becomes a notice; the palette's own refresh stays quiet.
    pub(super) fn refresh_docker(&mut self, explicit: bool) -> Task<Message> {
        let Some(program) = self.docker.program.clone() else {
            return Task::none();
        };
        if self.docker.loading
            || (!explicit && self.docker.loaded_at.is_some_and(|t| t.elapsed() < FRESH))
        {
            return Task::none();
        }
        self.docker.loading = true;
        background(
            move || forgeterm_docker::list(&program),
            move |result| Message::DockerListed(result, explicit),
        )
    }

    pub(super) fn docker_listed(
        &mut self,
        result: Result<Vec<forgeterm_docker::Container>, String>,
        explicit: bool,
    ) -> Task<Message> {
        self.docker.loading = false;
        self.docker.loaded_at = Some(std::time::Instant::now());
        match result {
            Ok(list) => {
                if explicit {
                    let running = list.iter().filter(|c| c.is_running()).count();
                    self.notice = Some(format!(
                        "Docker: {} container{}, {running} running. Their actions are in the command palette.",
                        list.len(),
                        if list.len() == 1 { "" } else { "s" }
                    ));
                }
                self.docker.containers = list;
            }
            Err(e) => {
                tracing::info!("docker: {e}");
                if explicit {
                    self.notice = Some(format!("Docker: {e}"));
                }
                self.docker.containers.clear();
            }
        }
        if self.command_palette.is_some() {
            let entries = self.palette_entries();
            if let Some(p) = &mut self.command_palette {
                p.set_entries(entries);
            }
        }
        Task::none()
    }

    /// A new tab with a shell in the container (`shell`) or its log.
    pub(super) fn docker_terminal(&mut self, key: u64, shell: bool) -> Task<Message> {
        let (Some(program), Some(c)) = (self.docker.program.clone(), self.docker_container(key))
        else {
            self.notice = Some("Docker: that container is no longer in the list.".to_owned());
            return Task::none();
        };
        let args = if shell {
            forgeterm_docker::shell_args(&c.id)
        } else {
            forgeterm_docker::logs_args(&c.id)
        };
        let args = match args {
            Ok(a) => a,
            Err(e) => {
                self.notice = Some(format!("Docker: {e}"));
                return Task::none();
            }
        };
        let name = if shell {
            format!("Docker: {}", c.name)
        } else {
            format!("Logs: {}", c.name)
        };
        let spec = ShellSpec::new(name, program.to_string_lossy(), args);
        let id = self.spawn_terminal(spec, Some(self.start_dir.clone()));
        self.workspace.push_tab(id);
        Task::none()
    }

    pub(super) fn docker_start_stop(&mut self, key: u64, start: bool) -> Task<Message> {
        let (Some(program), Some(c)) = (self.docker.program.clone(), self.docker_container(key))
        else {
            self.notice = Some("Docker: that container is no longer in the list.".to_owned());
            return Task::none();
        };
        let (id, name) = (c.id.clone(), c.name.clone());
        self.notice = Some(format!(
            "Docker: {} {name}…",
            if start { "starting" } else { "stopping" }
        ));
        background(
            move || {
                if start {
                    forgeterm_docker::start(&program, &id)
                } else {
                    forgeterm_docker::stop(&program, &id)
                }
            },
            move |result| {
                Message::DockerDone(
                    result,
                    format!("{} {name}", if start { "started" } else { "stopped" }),
                )
            },
        )
    }

    pub(super) fn docker_done(
        &mut self,
        result: Result<(), String>,
        what: String,
    ) -> Task<Message> {
        self.notice = Some(match result {
            Ok(()) => format!("Docker: {what}."),
            Err(e) => format!("Docker: {e}"),
        });
        self.docker.loaded_at = None;
        self.refresh_docker(false)
    }
}
