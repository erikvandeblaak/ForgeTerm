# ForgeTerm — Test Plan

## Automatisch (`cargo test --workspace`)

| Test | Wat | Status Win11 |
|---|---|---|
| `input::tests` (7) | Toets-encoding (ctrl, alt, DECCKM, modifiers, F-toetsen), muisrapporten SGR/legacy, paste-veiligheid (geen auto-submit, ESC-injectie) | ✅ |
| `palette::tests` (2) | 256-kleurenkubus, grijstinten, speciale kleuren | ✅ |
| `tests/pty_roundtrip.rs` | Echte shell (cmd / sh) in PTY: commando in, resultaat in grid, resize, exit-event | ✅ |
| `workspace::tests` (5) | Tabs toevoegen/selecteren/cyclen, splitsen geeft focus aan nieuwe pane, sluiten geeft focus aan buur, laatste pane sluit tab, actieve index blijft geldig | ✅ |
| `commands::tests` (5) | Geen conflicterende bindings; lookup; Ctrl+C, Ctrl+V, AltGr, Tab, Enter, pijltjes blijven bij de terminal; weergave "Ctrl+Shift+P" | ✅ |
| `fuzzy::tests` (5) | Subsequence, hoofdletterongevoelig, woordbegin wint ("spr" → Split Right), highlight-indices | ✅ |
| `palette::tests` (5) | Recent eerst, filteren/rangschikken, wrap, venster volgt selectie, vroege toetsaanslagen | ✅ |
| `tests/pty_roundtrip.rs::scrollback_scroll_and_clear` | 80 regels in echte shell → geschiedenis, naar boven/onder, wissen behoudt scherm | ✅ |
| `terminal_pane::tests` (3) | Tabtitels: exe-pad → shellnaam, `cmd.exe - ping …` → `ping …`, programmatitels blijven | ✅ |

Kwaliteitspoort voor iedere wijziging:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Handmatige / gescripte GUI-validatie

Uitgevoerd 2026-11-10 op Windows 11 (26300), PowerShell 7, met screenshots en gesimuleerde toets- en muisinvoer.

| # | Scenario | Verwacht | Resultaat |
|---|---|---|---|
| G1 | App start | Venster, pwsh-prompt in projectmap | ✅ |
| G2 | `Get-ChildItem`, `git --version` | Uitvoer, PSReadLine-syntaxkleuren | ✅ |
| G3 | ANSI 16/256/truecolor, bold/italic/underline/strike/dim/inverse | Correct gerenderd | ✅ |
| G4 | CJK dubbele breedte, combining accent | Uitlijning klopt | ✅ |
| G5 | Box-drawing (licht/zwaar/dubbel/afgerond), blokken | Naadloze randen | ✅ (dubbele hoeken licht overlappend) |
| G6 | `claude` | Trust-dialoog rendert, pijltjes werken, Esc sluit af | ✅ |
| G7 | vim (Git for Windows) | Alt screen, highlighting, `:q!` herstelt scherm | ✅ |
| G8 | Maximaliseren | Grid groeit (140×35 → 407×67), PTY volgt | ✅ |
| G9 | Ctrl+C tijdens `ping -n 30` | Onderbroken | ✅ |
| G10 | Scrollwiel | Scrollback, indicator `↑ n / total` | ✅ |
| G11 | Sleep-selectie + Ctrl+Shift+C | Markering, klembord bevat tekst | ✅ (nu met CRLF) |
| G12 | Plak 2 regels (Ctrl+Shift+V) | Bevestigingsbalk, Esc = niets uitgevoerd | ✅ |
| G13 | Plak 1 regel + newline | Ingevoegd, níet uitgevoerd | ✅ |
| G14 | `exit` → Enter | "Exited (0)", Enter herstart shell | ✅ |
| G15 | Venster sluiten | Geen weesprocessen (pwsh/conhost) | ✅ |

### Tabs en splits (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| S1 | Alt+Shift+= en Alt+Shift+- → 3 panes; invoer gaat naar gefocuste pane | ✅ |
| S2 | Focusrand cyaan, holle cursor in panes zonder focus | ✅ |
| S3 | Alt+← verplaatst focus; klik in pane focust | ✅ |
| S4 | Scheidingslijn slepen → grid van beide panes past zich aan (46×35) | ✅ |
| S5 | Ctrl+Shift+T, Ctrl+Shift+3 (cmd), **+**-knop, menu → Git Bash | ✅ |
| S6 | Ctrl+Tab cyclet, Alt+N selecteert | ✅ |
| S7 | `ping` in achtergrondtab loopt door; bij terugkeer volledige output | ✅ |
| S8 | `exit 3` → pane blijft met melding + amber stip; Ctrl+Shift+W sluit | ✅ |
| S9 | `exit` (code 0) in split → alleen die pane sluit | ✅ |
| S10 | × op middelste tab → tab weg, actieve tab blijft dezelfde | ✅ |
| S11 | Na sluiten: aantal shellprocessen = aantal panes (geen wezen) | ✅ |
| S12 | Claude Code in rechter split (68 kolommen), Esc sluit af | ✅ |
| S13 | Git Bash: kleurprompt met branch, `TERM=xterm-256color`, `ls --color` | ✅ |

Gevonden en opgelost tijdens deze tests: Ctrl+Alt+1 botst met AltGr (US-Intl `¡`) → Alt+1…9; cmd-titel bevatte exe-pad; focusrand werd door canvas overschilderd; shellmenu afgekapt.

### Command Palette (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| P1 | Ctrl+Shift+P opent, invoer gefocust, hints zichtbaar, alleen toepasbare commando's (20 bij 1 pane, 25 bij 2) | ✅ |
| P2 | "spr" → Split Right bovenaan met highlight; Enter splitst | ✅ |
| P3 | Recent gebruikt staat bovenaan; ↑↓ navigeert; teller "3 of 25" | ✅ |
| P4 | Snel typen direct na openen (`Ctrl+Shift+P zoom Enter` zonder pauze) | ✅ |
| P5 | Toggle Pane Zoom + statusbalk-indicator; Ctrl+Shift+Enter terug | ✅ |
| P6 | Clear Scrollback: geschiedenis weg, scherm blijft | ✅ |
| P7 | Restart Shell: nieuw pwsh-proces (PID 36104 → 39088), andere pane ongemoeid | ✅ |
| P8 | Klik buiten palette sluit zonder klik door te geven; klik op item voert uit | ✅ |
| P9 | Na sluiten gaat typen weer naar de terminal | ✅ |

Gevonden en opgelost: (1) de Enter die de palette bevestigde kwam óók in de nieuwe terminal (dubbele prompt); (2) Esc werd door het invoerveld afgevangen, waardoor de palette onzichtbaar "open" bleef en de volgende Ctrl+Shift+P hem sloot — getypte tekst ging dan naar de shell en werd uitgevoerd. Beide via de `captured`-status opgelost.

## Prestaties (release, Windows 11, i7 RaptorLake + RTX 4070 Laptop, 1200×760)

Gemeten 2026-11-10. Opstarttijden uit de eigen tracing-tijdstempels; geheugen = working set na 5 s, idle prompt.

| Meting | Waarde |
|---|---|
| Proces → venster zichtbaar | 213 ms |
| Proces → shell gestart | ~60 ms na eerste logregel |
| Proces → GPU-pipeline klaar | ~680 ms |
| Geheugen, Vulkan + dGPU + MSAA 4× (oude standaard) | 229 MB |
| Geheugen, Vulkan + iGPU, MSAA uit | 237–247 MB |
| Geheugen, **DX12 + iGPU, MSAA uit (nieuwe standaard)** | **147–165 MB** |
| Release-binary | 15 MB |

Beslissingen hieruit: `WGPU_POWER_PREF=low` en (Windows) `WGPU_BACKEND=dx12` als standaard, beide door de gebruiker te overschrijven; MSAA uit (terminal = rechte vlakken + tekst, visueel geen verschil).

Prestatiebudgetten (doel, nog te bewaken in CI): venster < 300 ms, idle geheugen < 175 MB, invoerlatentie < 1 frame.

### Testvalkuilen (geleerd)

- Start ForgeTerm voor tests **zonder** `NO_COLOR` in de omgeving (Claude Code zet `NO_COLOR=1`; PowerShell schakelt dan kleuren uit — correct gedrag, maar verwarrend).
- `SendKeys` met een US-International-layout maakt van `'`+`C` een `Ç` (dode toets) — geen ForgeTerm-bug.

## Nog te doen

- **Linux Mint 22**: bash/zsh/fish, X11 + Wayland, vim/nvim, ssh, Claude Code, Codex. ⬜
- Windows: Git Bash, WSL2, cmd, Windows PowerShell 5.1, Codex CLI, nvim, ssh. ⬜
- Langeduurtest (8 uur, geheugen stabiel). ⬜
- Throughput-benchmark (`cat` 100 MB, frames/latency). ⬜
- vttest-conformance. ⬜
