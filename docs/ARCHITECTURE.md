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
│   │   ├── src/shell.rs          # shelldetectie + ShellKind (pwsh, powershell, cmd, Git Bash, WSL / $SHELL, bash, zsh, fish)
│   │   ├── src/quote.rs          # paden quoten per shell (+ /c/… en /mnt/c/… conversie), unit-getest
│   │   ├── src/cwd.rs            # OSC 7 / OSC 9;9-scanner (werkmap-meldingen), unit-getest
│   │   ├── src/integration.rs    # shell-integratie: args/env zodat shells hun map melden
│   │   ├── tests/pty_roundtrip.rs# end-to-end test met echte shell
│   │   ├── tests/quote_real_shells.rs # gequote paden met lastige namen door elke geïnstalleerde shell laten lezen
│   │   ├── tests/cwd_tracking.rs # elke geïnstalleerde shell in PTY: cd → juiste map gemeld
│   │   └── examples/probe.rs     # diagnose: dump gestylede cellen van een programma
│   ├── forgeterm-explorer/       # bestandsboom-model, GUI-vrij, unit-getest
│   ├── forgeterm-agents/         # AI-agents: detectie, versie, start-commando, projectcontext; unit-getest
│   ├── forgeterm-settings/       # settings.toml: tolerant laden, template, pad; unit-getest
│   └── forgeterm-app/            # GUI (binary `forgeterm`)
│       ├── src/app.rs            # state, messages, update, layout, sneltoetsen
│       ├── src/workspace.rs      # tabs + split-layout (pane_grid-state), unit-getest
│       ├── src/commands.rs       # centrale command registry + keymap, unit-getest
│       ├── src/fuzzy.rs          # fuzzy matcher, unit-getest
│       ├── src/palette.rs        # Command Palette (logica getest + overlay-view)
│       ├── src/menu.rs           # menubalk: structuur uit de registry, navigatie (getest), dropdown
│       ├── src/explorer_view.rs  # zijpaneel van de bestandsverkenner
│       ├── src/terminal_pane.rs  # één terminal: sessie, snapshot, levenscyclus, titel
│       ├── src/terminal_view.rs  # canvas-renderer + muisafhandeling
│       ├── src/glyphs.rs         # pixel-exacte box-drawing/blok-tekens
│       ├── src/keys.rs           # iced-toetsen → engine-toetsen
│       └── src/theme.rs          # design tokens
├── assets/{fonts,branding}/
└── docs/
```

Geplande crates (zie ROADMAP): `forgeterm-platform`, `-prompts`, `-agents`, `-git`, `-analytics`, `-charts`, `-monitor-cli`, `-settings`, `-storage`. Ze worden pas aangemaakt wanneer er echte code in komt — geen lege crates.

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

## 4c. Commands, sneltoetsen en Command Palette

- `commands.rs` is de **centrale registry** (spec §9): elke actie is een `CommandId` met categorie, titel en key bindings (`Keymap`). Geen iced-types; unit-getest (o.a. "geen conflicterende bindings" en "Ctrl+C / AltGr / gewone toetsen worden nooit afgevangen").
- `App::run_command` is de **enige** plek waar acties worden uitgevoerd. Sneltoetsen, palette, knoppen en de menubalk sturen allemaal een `CommandId`.
- `App::is_applicable` bepaalt wat nu zin heeft; de palette toont alleen die commando's, zodat er nooit een optie verschijnt die niets doet.
- `fuzzy.rs`: subsequence-matcher met bonussen voor woordbegin en aaneengesloten tekens; levert indices voor highlighting.
- `palette.rs`: filter/rangschik/recent/zichtbaar-venster-logica (getest) + de overlay-view (`stack` + `opaque`, klik buiten = sluiten).
- **Menubalk** (`menu.rs`): menu's bevatten alleen `CommandId`s (een test eist dat elk command in een menu staat). Labels, sneltoetsen en de aan/uit-status worden bij het tekenen uit de registry, keymap en `is_applicable` gehaald; niet-toepasbare items zijn grijs en niet klikbaar. Titels hebben een vaste breedte, zodat de dropdown zonder tekstmeting exact onder zijn titel staat (`pin` in een `stack`). Een onzichtbare backdrop onder de topbalk sluit het menu bij klikken erbuiten zonder dat de klik doorgaat; de topbalk blijft klikbaar, dus hover over een andere titel wisselt van menu. Geen Alt+letter-mnemonics: Alt+F/B/… zijn readline-sneltoetsen van de shell.
- **Toetsroutering:** de globale toetsenlistener geeft per toets mee of een widget hem al heeft afgehandeld (`captured`). Afgevangen toetsen gaan nooit naar de terminal; Esc sluit de palette altijd; toetsen die binnenkomen vóórdat het invoerveld focus heeft, worden aan de zoekterm toegevoegd in plaats van verloren te gaan.

## 4d. Bestandsverkenner en paden invoegen

- `forgeterm-explorer::Tree` houdt root, uitgeklapte mappen, listings (Loading / Ready / Failed), verborgen-filter en selectie bij. Mappen worden pas gelezen bij uitklappen; `list_dir` draait in een eigen thread (`load_dir` in app.rs), zodat trage of netwerkschijven de UI nooit blokkeren. Resultaten voor mappen die inmiddels zijn ingeklapt of na een root-wissel binnenkomen, worden genegeerd (getest).
- Sortering: mappen eerst, daarna hoofdletterongevoelig. Verborgen = punt-bestanden en (Windows) het hidden-attribuut. Maximaal 2000 items per map; de rest wordt als aantal gemeld, niet stil weggelaten. Leesfouten verschijnen als regel in de boom.
- **Pad invoegen** (dubbelklik, *Insert path*, of OS-drag-and-drop via iced `window::Event::FileDropped`) gaat via `quote_path(shell.kind, …)` en daarna dezelfde weg als plakken (`App::insert_text`): nooit een Enter, meerregelig zonder bracketed paste vraagt bevestiging. Per shell: PowerShell `'…'` met verdubbelde (ook typografische) enkele quotes, cmd `"…"`, POSIX `'…'\''…'`, fish `\'`/`\\`, Git Bash `/c/…`, WSL `/mnt/c/…`.
- `ShellKind` wordt afgeleid uit het programmapad (System32\bash.exe = WSL, …\Git\…\bash.exe = Git Bash).

## 4e. Werkmap volgen (shell integration)

- **Bron:** shells melden hun map met OSC 7 (`file://host/pad`) of OSC 9;9 (Windows Terminal-stijl). vte/alacritty negeren beide, dus `ScanPty` wikkelt de PTY in: alle uitvoer gaat ongewijzigd door naar de emulator, terwijl `CwdScanner` (incrementeel, sequenties mogen over reads verdeeld zijn, max. 4 KB) meeleest. Alleen bestaande mappen worden gemeld (`is_dir` op de I/O-thread, zodat een trage netwerkschijf de UI niet raakt) als `SessionEvent::Cwd`.
- **Integratie zonder profielwijzigingen** (`integration.rs`): PowerShell krijgt na het laden van het profiel `-NoExit -Command` dat de bestaande `prompt` omwikkelt (OSC 9;9 met `ProviderPath`, alleen voor FileSystem). cmd: `PROMPT=$e]9;9;$P$e\…` vóór een bestaande PROMPT. Git Bash: `PROMPT_COMMAND` met de builtin `pwd -W` (Windows-pad; MSYS-paden als `/tmp` zijn anders niet te vertalen). Linux-bash: OSC 7 met `$PWD`. zsh/fish/WSL: geen injectie (zie ROADMAP).
- **Veiligheid:** een melding is een hint die elk programma kan printen. Ze bepaalt alleen waar nieuwe tabs/splits starten en wat de verkenner toont; nooit uitgevoerd.
- **App:** `TerminalPane.cwd`; nieuwe tab/split start in de map van de gefocuste shell, Restart Shell in de laatst gemelde map. `App::update` = `handle` + `sync_explorer` + `reveal_active_tab`: de verkenner volgt de gefocuste shell alleen als diens map veranderde sinds de vorige sync, zodat handmatig navigeren in de verkenner blijft staan tot de volgende `cd`. Volgen is uit te zetten (`cd`-knop, *Follow Terminal Folder*).

## 4f. Instellingen

- `forgeterm-settings::load` faalt nooit: ontbrekend bestand = defaults; syntaxfout, onbekende sleutel, verkeerd type of waarde buiten bereik → tekstuele melding in `problems`, die instelling houdt haar default. `unreadable` markeert een onleesbaar bestand.
- Geen serde: de TOML-tabel wordt met de hand uitgelezen, zodat elke afwijking een precieze melding krijgt in plaats van één harde fout.
- `App::load_settings(startup)`: bij start alles toepassen; bij herladen (mtime-check elke 2 s via `iced::time::every`, of *Reload Settings*) alleen wat in het bestand veranderde, zodat runtime-toggles (zoom, verkenner) niet door een ongerelateerde wijziging worden teruggezet. Een onleesbaar bestand tijdens het draaien houdt de huidige instellingen.
- Sessie-instellingen (scrollback, shell-integratie) worden per terminal bij aanmaken vastgelegd (`SessionOptions`); ze gelden voor nieuwe terminals.
- *Open Settings File* maakt het bestand alleen aan als het ontbreekt (`create_new`) en opent het in Kladblok (Windows; `.toml` heeft meestal geen koppeling) of via `xdg-open`.

## 4g. AI-werkplek (agent-adapters)

Alleen gedocumenteerde interfaces (Claude Code-documentatie op code.claude.com, geraadpleegd 2026-11-10: setup, cli-reference, memory, claude-directory, skills, sub-agents, settings, hooks, mcp, env-vars, terminal-config):

- **Detectie** (`forgeterm-agents::locate`): `claude` / `codex` op PATH (Windows: `.exe`, `.cmd`, `.bat`; `.ps1` overgeslagen), daarna `~/.local/bin` (native installer van Claude Code). `query_version`: `--version`, stdin dicht, eerste regel, timeout 20 s (virusscanners kunnen de eerste start vertragen), proces wordt bij timeout beëindigd. Alles op een achtergrondthread.
- **Starten** gebeurt alleen op een expliciete klik/command: `Start <agent> in Project` (tab of split) in de projectroot (dichtstbijzijnde map met `.git`, vanaf de map van de gefocuste shell). npm-shims (`.cmd`) via `cmd.exe /d /c`. Agent-terminals krijgen geen shell-integratie; een split vanuit een agent-pane opent een gewone shell. Omdat agents geen map melden, is de map van een pane (`TerminalPane::directory`) de laatst gemelde map, anders de startmap; AI-context, verkenner, statusbalk en nieuwe tab/split volgen daardoor ook bij een agent de projectroot.
- **Projectcontext** (alleen lezen): `CLAUDE.md`, `.claude/CLAUDE.md`, `CLAUDE.local.md`, `.claude/rules/*.md`, `AGENTS.md`, `.claude/skills/*/SKILL.md`, `.claude/commands/**/*.md`, `.claude/agents/*.md`, `.claude/settings(.local).json`, `.mcp.json`; `name`/`description` uit de front matter.
- **Invoegen**: klik op een contextbestand → in een Claude Code-terminal `@pad` (relatief aan de projectroot; met spaties het volledige pad), in Codex het volledige pad, in een shell het gequote pad. Nooit een Enter.
- **Niet gedaan / bewust niet:** ForgeTerm verstuurt geen prompts, leest geen sessies of tokens en toont geen kosten (geen gedocumenteerde bron). Hooks worden niet geïnterpreteerd (alleen het settings-bestand wordt getoond).

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

Linux is **nog niet gevalideerd** (geen Linux-machine getest). Wel compileert de hele workspace (incl. tests) voor `x86_64-unknown-linux-gnu` zonder clippy-waarschuwingen; daarbij bleek dat iced's `x11`/`wayland`-features ontbraken (de app zou op Linux niet gebouwd hebben) — nu toegevoegd. Zie TEST_PLAN.
