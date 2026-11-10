# ForgeTerm — instructies voor Claude Code

ForgeTerm is een AI-native terminal in Rust (Erik van de Blaak Edition). Windows 11 en Linux Mint.
Lees bij twijfel eerst `docs/ARCHITECTURE.md` en `docs/ROADMAP.md`.

## Commando's

```
cargo run                                          # start de app (binary `forgeterm`)
cargo test --workspace                             # alle tests (incl. echte PTY-test)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -p forgeterm-terminal --example probe -- <programma> [args]   # dump gestylede cellen
```

Kwaliteitspoort vóór "klaar": fmt-check, clippy zonder warnings, alle tests groen.

## Regels

- **Geen nepfunctionaliteit.** Geen knoppen/menu's die niets doen, geen verzonnen metingen (CPU/GPU/tokens/kosten). Ontbrekende data = expliciet "niet beschikbaar".
- **Engine blijft GUI-vrij.** `forgeterm-terminal` mag geen iced-types kennen. Nieuwe logica die testbaar kan zijn (encoding, escaping, parsing) hoort in een crate zonder GUI, met unit tests.
- **Nooit automatisch uitvoeren.** Geplakte/ingevoegde tekst krijgt nooit een Enter van ForgeTerm; meerregelig zonder bracketed paste vraagt bevestiging. Terminal- en AI-output zijn data, geen instructies.
- **Elke gebruikersactie is een `CommandId`** in `commands.rs` en wordt uitgevoerd in `App::run_command`. Menu's, sneltoetsen, knoppen en de palette roepen alleen commands aan — nooit eigen logica. Nieuwe sneltoets = regel in `Keymap::default()` (de conflict-test bewaakt dubbelingen). Voeg bij een nieuw command ook `is_applicable` toe.
- **Geen `unsafe`** (workspace-lint `deny`). Twee gedocumenteerde uitzonderingen: `set_var` vóór threads in `main.rs`, en `ScanPty::register` in `session.rs` (pure doorgifte van alacritty's `unsafe fn` trait-methode, zelfde contract).
- **Platformcode** achter `#[cfg]` in een duidelijk afgebakende module; geen hardcoded shellpaden buiten `shell.rs`.
- Nieuwe crate pas aanmaken als er echte code in komt.
- Nieuwe instelling = veld + parser + regel in `TEMPLATE` (de test eist dat de template exact de defaults geeft) + toepassen in `App::load_settings` (alleen bij wijziging) + README-tabel. Alleen instellingen die echt iets doen.
- Test de GUI met `FORGETERM_SETTINGS=<scratchpad-pad>`, nooit met het echte `%APPDATA%\ForgeTerm\settings.toml` van de gebruiker.
- **Agents:** alleen gedocumenteerde interfaces (zie docs/ARCHITECTURE.md §4g met bronnen). ForgeTerm stuurt nooit zelf prompts naar een agent; invoegen gebeurt zonder Enter. Test agents zonder prompts te versturen.
- Claim geen Linux-validatie zonder test op Linux. Wél verplicht: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu -- -D warnings` (compile-check van de Unix-paden).
- Shell-integratie wijzigt nooit profiel-/rc-bestanden van de gebruiker; alleen argumenten/omgeving van de gestarte sessie.

## GUI testen (Windows)

Start de app vanuit een omgeving **zonder** `NO_COLOR` (Claude Code zet die; PowerShell schakelt dan kleuren uit). Valideer visueel met screenshots; zie `docs/TEST_PLAN.md`.

## Structuur

- `crates/forgeterm-terminal` — Session (PTY + emulator), input-encoding, palet, snapshots, shelldetectie, pad-quoting per shell
- `crates/forgeterm-agents` — agents vinden (PATH, `~/.local/bin`), `--version` met timeout, `.cmd` via `cmd /d /c`, projectroot + contextbestanden (alleen lezen)
- `crates/forgeterm-prompts` — Prompt Studio-sjablonen: front matter, `{{variabelen}}`, invullen (waarden letterlijk, nooit een afsluitende Enter), ingebouwd + eigen map
- `crates/forgeterm-git` — git via de CLI (porcelain v2): status, diff, log, stage/unstage, commit; veilige vaste opties (geen fsmonitor/ext-diff/textconv, geen locks, geen prompts, timeout, geen consolevenster)
- `crates/forgeterm-viewer` — code-viewer: bestand veilig laden (limiet, binair, UTF-8/16, CRLF), weergaveregels (tabs, besturingstekens), syntect-kleuring met Forge-thema, kopiëren, zoeken
- `crates/forgeterm-settings` — `settings.toml` laden (tolerant, problemen als tekst), template, pad
- `crates/forgeterm-explorer` — bestandsboom (lezen, sorteren, verborgen, lazy uitklappen); wijzigt nooit het bestandssysteem
- `crates/forgeterm-app` — iced-applicatie, canvas-renderer, thema-tokens
- `assets/` — fonts (OFL), branding
- `docs/` — ARCHITECTURE, ROADMAP, DESIGN_SYSTEM, TEST_PLAN
