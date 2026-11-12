# Changelog

All notable changes to ForgeTerm. Dates are release dates.

## [0.1.1] – 2026-11-11

### Added
- **Shell integration.** PowerShell, bash and cmd mark every prompt with a bar in the margin (succeeded, failed, running). Ctrl+Shift+↑ and ↓ jump between commands. Copy the output of the last command, or put it in your agent's prompt without Enter.
- **Tabs that ask for attention.** A tab gets a badge when a program is waiting for you, is done, has failed or rings the bell. When the window is in the background, the taskbar asks for attention.
- **Open file references.** Ctrl+click on a reference in the output, such as `src/main.rs:42` or a line from a Python traceback, opens the file in the code viewer at that line. ForgeTerm only reads the file and runs nothing.
- **Explorer by keyboard.** Ctrl+Shift+E puts the focus in the explorer: arrow keys, PgUp/PgDn, Home/End and Enter. The tree refreshes by itself when a folder changes, and *Go to Folder* jumps to a typed or pasted path.
- A built-in Prompt Studio template that asks the agent to check its work thoroughly and then continue.

## [0.1.0] – 2026-11-10

First public build: GPU-rendered terminal (ConPTY / PTY with alacritty_terminal), tabs and splits, Command Palette and menu bar, file explorer, code viewer with minimap, Git panel, AI workspace for Claude Code and Codex, Prompt Studio, monitor, session restore, `settings.toml` with live reload. Windows installer, portable zip and a `.deb` for Linux Mint 22 / Ubuntu 24.04.

[0.1.1]: https://github.com/erikvandeblaak/ForgeTerm/releases/tag/v0.1.1
[0.1.0]: https://github.com/erikvandeblaak/ForgeTerm/releases/tag/v0.1.0
