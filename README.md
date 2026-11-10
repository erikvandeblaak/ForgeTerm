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
| **Command Palette** (alle acties, fuzzy zoeken) | **Ctrl+Shift+P** |
| Nieuwe tab (standaardshell) | Ctrl+Shift+T, of **+** in de tabbalk |
| Nieuwe tab met shell N (zie menu *New tab with…*) | Ctrl+Shift+1 … 9 |
| Naar tab N | Alt+1 … 9 |
| Volgende / vorige tab | Ctrl+Tab / Ctrl+Shift+Tab |
| Splitsen naast elkaar / onder elkaar | Alt+Shift+= / Alt+Shift+- |
| Focus naar pane links/rechts/boven/onder | Alt+pijltjes, of klik in de pane |
| Pane (of tab) sluiten | Ctrl+Shift+W, of **×** op de tab |
| Pane-grootte | Sleep de scheidingslijn |
| Pane maximaliseren / terug | Ctrl+Shift+Enter |
| Shell herstarten, scrollback wissen, naar begin/eind scrollen | via de Command Palette of het menu Terminal |
| Menubalk (File, Edit, View, Terminal, Help) | klik op een titel; daarna ←→ ↑↓ Enter Esc |
| Bestandsverkenner tonen/verbergen | **Ctrl+Shift+E** |
| Instellingen openen (`settings.toml`, wordt live toegepast) | **Ctrl+,** of File → Open Settings File |
| AI-werkplek (Claude Code / Codex starten, projectcontext) | **Ctrl+Shift+A** of menu AI |
| Pad in de terminal zetten (gequote voor de shell, níet uitgevoerd) | dubbelklik op bestand, knop *Insert path*, of sleep een bestand uit Windows Verkenner op het venster |
| Kopiëren | Ctrl+Shift+C, of Ctrl+C als er een selectie is |
| Plakken | Ctrl+Shift+V, Shift+Insert, rechtermuisklik |
| Scrollback | Muiswiel, Shift+PageUp / Shift+PageDown |
| Lettergrootte | Ctrl+= / Ctrl+- / Ctrl+0 |
| Shell herstarten na een fout-exit | Enter (een schone `exit` sluit de pane) |

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

## Instellingen

ForgeTerm leest `%APPDATA%\ForgeTerm\settings.toml` (Linux: `$XDG_CONFIG_HOME/forgeterm/settings.toml`, anders `~/.config/forgeterm/`). Met `FORGETERM_SETTINGS=<pad>` kies je een ander bestand. **Ctrl+,** maakt het bestand (met uitleg per instelling) aan als het nog niet bestaat en opent het (Windows: Kladblok; Linux: `xdg-open`); opgeslagen wijzigingen worden binnen ~2 seconden toegepast.

| Instelling | Standaard | Betekenis |
|---|---|---|
| `terminal.font_size` | 14 | 8–32 |
| `terminal.scrollback` | 10000 | regels per terminal (0–100000), voor nieuwe terminals |
| `terminal.default_shell` | eerste gevonden | naam zoals in "New tab with…", bv. `"Git Bash"` |
| `terminal.start_directory` | startmap van ForgeTerm | map van de eerste tab |
| `terminal.shell_integration` | true | shells melden hun map (geen profielwijzigingen) |
| `explorer.visible` / `follow_terminal` / `show_hidden` | false / true / false | verkenner |

Fouten (typefout, onbekende sleutel, waarde buiten bereik) verschijnen in een balk bovenin; die instelling valt terug op de standaard. Een syntaxfout tijdens het bewerken laat de huidige instellingen staan. ForgeTerm schrijft nooit in een bestaand instellingenbestand.
