# Contributing to ForgeTerm

Thanks for taking the time. Bug reports, ideas and pull requests are welcome.

## Reporting a bug

Open an [issue](https://github.com/erikvandeblaak/ForgeTerm/issues/new/choose) with:

- your platform (Windows 11 build, or Linux distribution and desktop) and the ForgeTerm version (*Help › About*);
- the shell (PowerShell 7, Windows PowerShell, cmd, Git Bash, WSL, bash) and, if relevant, the program you ran (vim, Claude Code, Codex …);
- what you did, what you expected and what happened instead.

Security problems go privately, see [SECURITY.md](SECURITY.md).

## Pull requests

1. Open an issue first for anything bigger than a small fix, so we can agree on the approach.
2. Keep the change focused. One feature or fix per pull request.
3. Run the quality gate before you push:

   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

   On Windows also check the Linux code paths compile:
   `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`.

## Ground rules

These are the rules the code follows. A pull request that breaks one of them will not be merged.

- **No fake functionality.** No buttons or menus that do nothing, no invented measurements. Missing data is shown as unavailable.
- **The terminal engine stays GUI-free.** `forgeterm-terminal` does not know iced types. Testable logic (encoding, escaping, parsing) goes in a crate without GUI, with unit tests.
- **Never execute automatically.** Pasted or inserted text never gets an Enter from ForgeTerm. Terminal and AI output is data, not instructions. ForgeTerm never sends a prompt to an agent by itself.
- **Every user action is a `CommandId`** in `commands.rs`, run through `App::run_command`. Menus, shortcuts, buttons and the palette only call commands.
- **No `unsafe`** (workspace lint `deny`), apart from the two documented exceptions.
- **Platform code** sits behind `#[cfg]` in a clearly bounded module.
- **A new setting** needs a field, a parser, a line in the template, applying it in `App::load_settings`, and a row in the README table.
- **Shell integration never changes** the user's profile or rc files.

More background in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License of contributions

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in ForgeTerm by you, as defined in the Apache-2.0 license, shall be dual licensed under the MIT and Apache-2.0 licenses, without any additional terms or conditions.
