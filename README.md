# ForgeTerm

**AI-Native Developer Workspace** — Erik van de Blaak Edition
Created by Erik van de Blaak.

Status: eerste werkende build (Windows 11). Echte ConPTY-terminal met GPU-rendering; Claude Code, vim en PowerShell draaien erin. Zie [docs/ROADMAP.md](docs/ROADMAP.md) voor wat wel en niet af is.

## Starten

```
cargo run --release
```

Vereist: Rust (stable) en op Windows de Visual Studio C++ Build Tools + Windows SDK.

## Sneltoetsen

| Actie | Toets |
|---|---|
| Kopiëren | Ctrl+Shift+C, of Ctrl+C als er een selectie is |
| Plakken | Ctrl+Shift+V, Shift+Insert, rechtermuisklik |
| Scrollback | Muiswiel, Shift+PageUp / Shift+PageDown |
| Lettergrootte | Ctrl+= / Ctrl+- / Ctrl+0 |
| Shell herstarten na exit | Enter |

## Ontwikkelen in VS Code

Open de map in VS Code; aanbevolen extensies worden voorgesteld (`.vscode/extensions.json`).

| Actie | Hoe |
|---|---|
| Bouwen | `Ctrl+Shift+B` |
| Debuggen | `F5` → *ForgeTerm — Debug (MSVC)* |
| Tests | *Tasks: Run Test Task*, of de ▶-lens boven een test |
| Lint | Task *cargo clippy (workspace)* — rust-analyzer draait clippy ook bij opslaan |

Opslaan formatteert automatisch (rustfmt). rust-analyzer gebruikt een eigen target-map, zodat het de build niet blokkeert.

## Documentatie

- [Architectuur](docs/ARCHITECTURE.md)
- [Roadmap & bekende beperkingen](docs/ROADMAP.md)
- [Design system](docs/DESIGN_SYSTEM.md)
- [Testplan](docs/TEST_PLAN.md)

## Licenties van derden

JetBrains Mono — SIL Open Font License 1.1 (`assets/fonts/JetBrainsMono-OFL.txt`).
Iced (MIT), alacritty_terminal (Apache-2.0).
