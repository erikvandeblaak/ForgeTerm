# Changelog

All notable changes to ForgeTerm. Dates are release dates.

## [0.2.1] – 2026-11-17

### Fixed
- **Git Control Center:** the WORKTREES tile counted the main worktree as a linked one ("3 linked worktrees in use" with two linked worktrees). It now reads, for example, *main + 2 linked* ([#1](https://github.com/erikvandeblaak/ForgeTerm/issues/1)).

## [0.2.0] – 2026-11-17

### Added
- **Docker** (when the `docker` CLI is installed): the containers appear in the command palette. Open a shell in a running container or follow its log, each in its own tab, and start or stop a container. ForgeTerm never removes containers, images or volumes, and without a running engine it says so.
- **Hooks and MCP servers in the AI panel:** the hooks in `.claude/settings.json` / `settings.local.json` with the event, matcher and the command they run (in amber), and the servers in `.mcp.json` with their command or url. ForgeTerm only shows them and never runs them.
- **Mission Control** (Ctrl+Shift+D): a live card for every agent in the window, with status (from the agent itself, the bell or the process), project, branch, runtime, last output, a live miniature and measured CPU and memory. Open, Files, Changes and Stop from the card, plus a list of ended sessions.
- **Git Control Center** (Ctrl+Shift+H): branch, upstream, staged, unstaged and untracked files, worktrees, recent commits and activity on one page.
- **An agent in its own worktree:** start Claude Code or Codex on a new branch in a new worktree, and remove the worktree again when you are done. Removal refuses while a terminal still works in it.
- **Compare Worktrees** (Ctrl+Shift+K): the changes of two worktrees since their common commit, side by side, with test terminals in both and one prompt for both agents (inserted without Enter).
- **Five color themes:** Midnight (the original look), Graphite, Arctic (light), Cyber and High Contrast. Set one with `[appearance] theme` in `settings.toml`, or try one with *View → Theme*. Switching needs no restart, and every theme's contrast is tested.
- **GPU usage in the monitor:** NVIDIA GPUs through the driver (NVML), and AMD GPUs on Linux through sysfs. Load, VRAM and temperature, measured only while the panel is open. Other GPUs show *not available* with the reason.
- **SSH profiles:** the hosts of your `~/.ssh/config` appear as *New SSH Tab* in the palette, the File menu and the new-tab picker. The tab runs your own OpenSSH client, and ForgeTerm never sees a password.
- **Commands** for closing a tab, inserting, copying and reloading a prompt, committing, staging or unstaging everything, trusting a repository and showing remote branches. They are in the palette and the menus.

### Changed
- An SSH or container tab no longer shows the local folder's Git status in the status bar.
- **Search in the terminal** waits for a short pause in typing, so large scrollbacks no longer slow down every key press.
- **Background work:** Git status and the settings check pause while the window has no focus.
- **Side panels:** only one is open at a time.
- **Shortcut hints** in the screens follow your own `[keys]`.
- **Shortcuts that do not apply** at that moment do nothing.

### Fixed
- **PowerShell 7 with `NO_COLOR` set** lost its command marks, so copying the last output, jumping between commands and exit codes did not work.
- **About and Keyboard Shortcuts** opened from the command palette closed again at once.
- **A failed session save** now shows one clear message, instead of a log line every two seconds.
- **A network folder as start folder** can no longer make the window hang.
- **A refused branch name** stays in the field so you can correct it.
- **Security:**
  - Git submodules respect the repository trust check.
  - The code viewer only opens regular files.
  - Links to network shares through symlinks are not followed.

## [0.1.1] – 2026-11-11

### Added
- **Shell integration.** PowerShell, bash and cmd mark every prompt with a bar in the margin (succeeded, failed, running). Ctrl+Shift+↑ and ↓ jump between commands. Copy the output of the last command, or put it in your agent's prompt without Enter.
- **Tabs that ask for attention.** A tab gets a badge when a program is waiting for you, is done, has failed or rings the bell. When the window is in the background, the taskbar asks for attention.
- **Open file references.** Ctrl+click on a reference in the output, such as `src/main.rs:42` or a line from a Python traceback, opens the file in the code viewer at that line. ForgeTerm only reads the file and runs nothing.
- **Explorer by keyboard.** Ctrl+Shift+E puts the focus in the explorer: arrow keys, PgUp/PgDn, Home/End and Enter. The tree refreshes by itself when a folder changes, and *Go to Folder* jumps to a typed or pasted path.
- A built-in Prompt Studio template that asks the agent to check its work thoroughly and then continue.

## [0.1.0] – 2026-11-10

First public build: GPU-rendered terminal (ConPTY / PTY with alacritty_terminal), tabs and splits, Command Palette and menu bar, file explorer, code viewer with minimap, Git panel, AI workspace for Claude Code and Codex, Prompt Studio, monitor, session restore, `settings.toml` with live reload. Windows installer, portable zip and a `.deb` for Linux Mint 22 / Ubuntu 24.04.

[0.2.1]: https://github.com/erikvandeblaak/ForgeTerm/releases/tag/v0.2.1
[0.2.0]: https://github.com/erikvandeblaak/ForgeTerm/releases/tag/v0.2.0
[0.1.1]: https://github.com/erikvandeblaak/ForgeTerm/releases/tag/v0.1.1
[0.1.0]: https://github.com/erikvandeblaak/ForgeTerm/releases/tag/v0.1.0
