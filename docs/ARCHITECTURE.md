# ForgeTerm — Architecture

Status: **Fase 1 (Fundament) — verticale slice werkt op Windows 11.**
Laatste update: 2026-11-10.

## 1. Doel van de architectuur

Drie dingen moeten los van elkaar ontwikkeld en getest kunnen worden:

1. **Terminalengine** — PTY + VT-emulatie, geen GUI-afhankelijkheden.
2. **Datavisualisatie / monitoring** — metrics-verzameling los van presentatie (GUI-dashboard én `forge-monitor` CLI).
3. **AI-integraties** — agent-adapters die alleen via gedocumenteerde CLI-interfaces werken.

De GUI is een dunne laag die deze crates combineert.

## 2. Technologiekeuzes (gevalideerd)

| Onderdeel | Keuze | Versie | Licentie | Waarom |
|---|---|---|---|---|
| UI | **Iced** | 0.14 | MIT | Pure Rust, wgpu-GPU-rendering met tiny-skia fallback, `canvas` voor eigen terminal- en grafiekrendering, Elm-architectuur (voorspelbare state). |
| VT-emulatie | **alacritty_terminal** | 0.26 | Apache-2.0 | Bewezen emulator (Alacritty, Zed). Volledige VT/xterm, truecolor, alt screen, scrollback, selectie, OSC 52. |
| PTY | **alacritty_terminal::tty** | 0.26 | Apache-2.0 | ConPTY op Windows, echte PTY op Unix, met eigen I/O-thread (`EventLoop`). Daardoor is `portable-pty` niet nodig. |
| Logging | tracing + tracing-subscriber | 0.1 / 0.3 | MIT | Gestructureerde logs, `RUST_LOG`-filter. |
| Fouten | anyhow / thiserror | 1 / 2 | MIT/Apache | anyhow in de app, thiserror voor publieke crate-fouten. |
| Font | JetBrains Mono 2.304 | — | OFL-1.1 | Gebundeld in `assets/fonts`, licentie meegeleverd. |

**Afgewezen:** *Slint* (1.18) — technisch geschikt, maar de licentie (GPLv3 / royalty-free met attributie / commercieel) is minder vrij dan MIT voor een commercieel bedoeld product. *iced_term* (0.8) — kant-en-klare terminalwidget, maar we willen controle over rendering, input-encoding en beveiliging (paste, OSC 52) en een UI-onafhankelijke engine. *portable-pty* — overbodig naast de PTY-laag van alacritty_terminal.

## 3. Workspace

```text
ForgeTerm/
├── Cargo.toml                    # workspace, gedeelde deps/lints/profielen
├── crates/
│   ├── forgeterm-terminal/       # ENGINE — geen GUI-types
│   │   ├── src/session.rs        # Session: PTY + Term + I/O-thread, snapshots, selectie, resize
│   │   ├── src/input.rs          # key/mouse/paste → bytes (xterm-conventies), unit-getest
│   │   ├── src/palette.rs        # kleurresolutie (16/256/truecolor + OSC-overrides)
│   │   ├── src/snapshot.rs       # Snapshot: render-klare kopie van de viewport
│   │   ├── src/shell.rs          # shelldetectie (pwsh, powershell, cmd, Git Bash, WSL / $SHELL, bash, zsh, fish)
│   │   ├── tests/pty_roundtrip.rs# end-to-end test met echte shell
│   │   └── examples/probe.rs     # diagnose: dump gestylede cellen van een programma
│   └── forgeterm-app/            # GUI (binary `forgeterm`)
│       ├── src/app.rs            # state, messages, update, layout, sneltoetsen
│       ├── src/workspace.rs      # tabs + split-layout (pane_grid-state), unit-getest
│       ├── src/terminal_pane.rs  # één terminal: sessie, snapshot, levenscyclus, titel
│       ├── src/terminal_view.rs  # canvas-renderer + muisafhandeling
│       ├── src/glyphs.rs         # pixel-exacte box-drawing/blok-tekens
│       ├── src/keys.rs           # iced-toetsen → engine-toetsen
│       └── src/theme.rs          # design tokens
├── assets/{fonts,branding}/
└── docs/
```

Geplande crates (zie ROADMAP): `forgeterm-platform`, `-explorer`, `-prompts`, `-agents`, `-git`, `-analytics`, `-charts`, `-monitor-cli`, `-settings`, `-storage`. Ze worden pas aangemaakt wanneer er echte code in komt — geen lege crates.

## 4. Dataflow terminal

```text
 toetsenbord/muis (iced)                         I/O-thread (alacritty EventLoop)
        │                                               │
   keys.rs / terminal_view.rs                           │ leest PTY-output
        │  Key/MouseButton                              ▼
   input.rs: encode_* ──► Session::input/write ──► Msg::Input ──► PTY (ConPTY/Unix PTY) ──► shell
                                                        │
                                       VT-parser ► Term (grid, scrollback, modes)
                                                        │ Event::Wakeup (gecoalesced)
        ┌───────────────────────────────────────────────┘
        ▼
  Listener-sink ► futures mpsc ► Task::run ► Message::Session(gen, Wakeup)
        │
  App::refresh ► Session::snapshot(palette) ► Snapshot ► Cache::clear ► canvas redraw
```

Belangrijke details:

- **Wakeup-coalescing:** de listener stuurt maximaal één `Wakeup` tot de UI een snapshot neemt (`AtomicBool`). Bij een outputstorm (bijv. `cat` van een groot bestand) wordt de UI niet overspoeld.
- **Antwoorden aan programma's** (`PtyWrite`: device attributes, cursorpositie; kleur- en grootte-queries) worden direct vanuit de listener teruggeschreven, zonder UI-roundtrip — essentieel voor TUI's zoals Claude Code, vim en fzf.
- **Generaties:** iedere (her)start krijgt een generatienummer; events van een oude sessie worden genegeerd.
- **Resize:** de canvas vergelijkt bij iedere redraw zijn echte afmetingen met het huidige grid en publiceert `GridResized`. Zo klopt het grid ook als er later panelen bijkomen (de paste-balk bewijst dit al: 140×35 → 140×32).

## 4b. Tabs en splits

- `App.terminals: HashMap<TermId, TerminalPane>` bezit alle terminals; `Workspace { tabs: Vec<Tab>, active }` beschrijft alleen de indeling. Een `Tab` is een iced `pane_grid::State<TermId>` plus de gefocuste pane. Zo kan een terminal later tussen panes/tabs verhuizen zonder zijn sessie te verliezen.
- Elk bericht van een canvas draagt zijn `TermId` (`Message::Term(id, TermMsg)`); sessie-events dragen `(id, generation)`.
- **Onzichtbare terminals renderen niet.** Een `Wakeup` voor een terminal buiten de actieve tab zet alleen `dirty`; bij tabwissel worden dirty terminals ververst. Output gaat nooit verloren (die staat in de emulator), alleen het kopiëren naar een snapshot wordt uitgesteld.
- **Afsluiten:** exitcode 0 sluit de pane (en tab, en app als het de laatste was); een andere exitcode laat de pane met melding staan (Enter = herstart).
- Focus bepaalt de cursorstijl (blok vs. hol), dus bij iedere focuswissel worden de caches van de actieve tab gewist.

## 5. Rendering

- Eén `canvas::Cache`, gewist bij iedere nieuwe snapshot.
- Achtergronden: runs van gelijke kleur → één rechthoek.
- Tekst: ASCII-runs met identieke stijl als één string (monospace houdt uitlijning); alle overige tekens per cel op exacte gridpositie (CJK, emoji, combining).
- Box-drawing (U+2500–257F) en blokelementen (U+2580–259F) worden als geometrie getekend zodat randen naadloos aansluiten ongeacht regelhoogte.
- Celmaat: breedte = 0,6 em (JetBrains Mono advance), hoogte = ceil(1,35 em).

## 6. Beveiligingsbeslissingen (al geïmplementeerd)

| Risico | Maatregel |
|---|---|
| Plakken voert commando's uit | Afsluitende regeleinden worden altijd gestript. Meerregelig plakken zonder bracketed paste vraagt expliciete bevestiging (balk, Enter/Esc). |
| Paste-injectie via `ESC[201~` | ESC wordt uit bracketed-paste-inhoud verwijderd (unit test). |
| Klembord uitlezen door programma's (OSC 52 query) | Nooit beantwoord. Alleen schrijven (OSC 52 store) wordt doorgegeven. |
| Terminaloutput als instructie | Output wordt uitsluitend gerenderd; nooit geïnterpreteerd als app-commando. |
| Unsafe code | `unsafe_code = "deny"` op workspace-niveau. |

## 7. Platformgrenzen

Alle platformspecifieke code zit nu in `shell.rs` (`#[cfg(windows)]` / `#[cfg(unix)]`) en in alacritty's `tty`. Bij het eerste platformspecifieke onderdeel buiten de engine (GPU-metrics, bestandsbeheer) komt `forgeterm-platform` met traits per functie.

Linux is **nog niet gevalideerd** (geen Linux-machine getest). De code compileert alleen met de Unix-paden in theorie; zie TEST_PLAN.
