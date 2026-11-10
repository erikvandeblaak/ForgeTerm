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
- **Geen `unsafe`** (workspace-lint `deny`).
- **Platformcode** achter `#[cfg]` in een duidelijk afgebakende module; geen hardcoded shellpaden buiten `shell.rs`.
- Nieuwe crate pas aanmaken als er echte code in komt.
- Claim geen Linux-validatie zonder test op Linux.

## GUI testen (Windows)

Start de app vanuit een omgeving **zonder** `NO_COLOR` (Claude Code zet die; PowerShell schakelt dan kleuren uit). Valideer visueel met screenshots; zie `docs/TEST_PLAN.md`.

## Structuur

- `crates/forgeterm-terminal` — Session (PTY + emulator), input-encoding, palet, snapshots, shelldetectie
- `crates/forgeterm-app` — iced-applicatie, canvas-renderer, thema-tokens
- `assets/` — fonts (OFL), branding
- `docs/` — ARCHITECTURE, ROADMAP, DESIGN_SYSTEM, TEST_PLAN
