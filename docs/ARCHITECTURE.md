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
│   ├── forgeterm-prompts/        # Prompt Studio-sjablonen: parsen, variabelen, invullen; unit-getest
│   ├── forgeterm-git/            # Git via de CLI: status (porcelain v2), diff, log, stage/unstage, commit; tests tegen echte git
│   ├── forgeterm-viewer/         # Code-viewer: laden, decoderen, kleuren (syntect, pure-Rust regex), zoeken; unit-getest
│   ├── forgeterm-system/         # Procesinfo (sysinfo): programma's die een shell nog draait; tests met echte processen
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
- **Start op de echte grootte.** Een nieuwe pane start zijn shell pas bij de eerste layout (`TerminalPane::resize`), niet op een 80×24-placeholder: een shell die zijn prompt tekent en direct daarna wordt verkleind/vergroot (ConPTY-reflow) heeft een verkeerde cursorpositie, waardoor PowerShell over zijn prompt typte.
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

## 4h. Prompt Studio

- **Sjablonen** (`forgeterm-prompts`): Markdown met optionele front matter (`name`, `description`) en `{{naam}}`-placeholders (`[A-Za-z0-9_-]`, spaties binnen de haken toegestaan; al het andere blijft letterlijke tekst). Ingebouwd: 6 sjablonen (`include_str!`). Eigen: `.md` in `<settings-map>/prompts` (niet recursief, max 256 KB, UTF-8; problemen als tekst), overschrijven een ingebouwd sjabloon met dezelfde bestandsnaam. Laden op een achtergrondthread, bij elke opening en via Reload.
- **Invullen** (`render`): waarden worden letterlijk ingevoegd (niet opnieuw op placeholders gescand), lege waarden tellen als ontbrekend en blijven `{{naam}}`; het resultaat eindigt nooit op een newline. *Insert* is uitgeschakeld zolang er iets ontbreekt.
- **Automatische waarden** alleen uit echte state: projectroot (`project_root` vanaf de map van de gefocuste terminal), die map, de terminalselectie en het in de verkenner geselecteerde bestand. Onbekend = niet ingevuld.
- **Invoegen** gaat naar de terminal die gefocust was bij het openen, via hetzelfde pad als plakken (bracketed paste; zonder bracketed paste + meerdere regels → bevestiging). Is die terminal intussen gestopt, dan blijft de studio open met alleen Copy.
- **Toetsenbord**: de studio is modaal; editors krijgen typen en Ctrl+C/V/X, Tab/Shift+Tab wisselt veld, Esc sluit. Niets bereikt de terminal.

## 4i. Git

- **Via de `git`-CLI** met documenteerde scriptformaten (`status --porcelain=v2 --branch -z`, `log --format`), geen libgit2: dezelfde config, hooks en credential helpers als de gebruiker.
- **Vaste veiligheidsopties** (`forgeterm-git::run`): `--no-optional-locks`, `-c core.fsmonitor=false` (een kwaadaardige repo-config kan anders een programma laten draaien bij `status`; getest met een echte hook als controle), `-c color.ui=false`, `-c core.pager=cat`, diffs met `--no-ext-diff --no-textconv`; `GIT_TERMINAL_PROMPT=0`, stdin dicht, beide uitvoerstromen uitgelezen, timeout (15 s lezen, 120 s commit), `CREATE_NO_WINDOW` op Windows. `git` wordt via PATH gevonden; Rust zoekt op Windows niet in de huidige map, dus een `git.exe` in een repository wordt niet gestart.
- **Verversen** op een achtergrondthread: bij elke mapwissel van de gefocuste terminal (`sync_git`, ontdekt de root opnieuw), elke 4 s voor dezelfde map (bekende root, één git-aanroep; overgeslagen als er nog een loopt), en na elke actie. Resultaten voor een map die de terminal intussen verliet worden weggegooid. Het log alleen als het paneel open is.
- **Schrijvende acties** alleen op een klik: stage (`git add`), unstage (`git restore --staged`; vóór de eerste commit `git rm --cached`), commit (`git commit -m`, inclusief hooks). Conflicten worden niet via het paneel gestaged (dat markeert ze als opgelost). Geen discard, push, pull of branchwissel: die horen bewust in de terminal.
- **Branches** (`refs.rs`): `for-each-ref` met NUL-gescheiden velden (naam, HEAD, upstream, ahead/behind of `gone`, datum, onderwerp), `origin/HEAD` overgeslagen. Wisselen = `git switch --no-guess <naam>` na bevestiging in de UI; nieuwe branch = `git check-ref-format --branch` en dan `git switch -c`; volgen = `git switch --track`. Namen die met `-` beginnen worden geweigerd (geen optie-injectie).
- **History** (`history.rs`): `git log --format` met %x1f/%x1e-scheiding (300 commits), `git show --format=fuller --stat --patch` (alleen hex-hash geaccepteerd), activiteit = `git log --since=<unix>` → authored-tijden → lokale kalenderdag via `chrono` (`daily_counts` is puur en getest). Grafiekkleur `CHART_CYAN` (#26A6B6) gevalideerd met de dataviz-validator tegen het donkere oppervlak; één reeks dus geen legenda, tooltip per staaf (datum + aantal), alleen max en de datumuiteinden gelabeld.
- **Lijsten** max 150 rijen per sectie, diffs max 5000 regels (gemarkeerd als afgekapt).

## 4j. Code-viewer

- **Laden** (`forgeterm-viewer::load`, achtergrondthread): max 8 MB; NUL in de eerste 8 KB zonder UTF-16-BOM = binair (geweigerd); UTF-8, UTF-8-BOM, UTF-16 LE/BE, anders lossy (zichtbaar in de statusregel). Twee versies per regel: het origineel (kopiëren) en een weergaveregel (tabs naar 4 kolommen, besturingstekens naar U+2400-symbolen, afgekapt op 2000 kolommen). Een ESC-reeks in een bestand wordt dus getoond, nooit geïnterpreteerd.
- **Kleuren** met syntect (standaard-syntaxen, `regex-fancy`: geen C-afhankelijkheid, bouwt gelijk op Linux) en een eigen thema uit de Forge-tokens; tot 2 MB / 50.000 regels, op een achtergrondthread (≈0,2 s voor 2.500 regels), daarna pas getoond (versie-check op mtime). Syntax op extensie, een paar aliassen (Makefile, Dockerfile, jsonc, mjs), daarna op de eerste regel (shebang).
- **Weergave**: eigen canvas (zoals de terminal) met regelnummers, selectie per regel, zoektreffers en een minimap (2 px per regel; bij lange bestanden schuift de minimap proportioneel mee). Klik in de minimap centreert die regel, slepen werkt als scrollbalk (fractie, geen terugkoppeling). Wijzigingen op schijf: mtime-check elke 2 s, herladen houdt de positie.
- **Toetsenbord**: alleen als de viewer de focus heeft (klik erin); app-sneltoetsen blijven werken, de terminal krijgt dan niets. Klikken in een terminal geeft de focus terug.

## 4j2. Zoeken in de terminal

`Session::search` gebruikt alacritty's regex-zoeker (scherm + scrollback, loopt rond aan de randen, smart case) met een letterlijk geëscapete zoektekst (`search::escape`: elke ASCII-leesteken krijgt een backslash). Start naast de vorige treffer (de huidige selectie) of bij de cursor; de treffer wordt de selectie (dus Ctrl+C kopieert hem) en de weergave scrollt ernaartoe. Zoekbalk Ctrl+Shift+F: typen zoekt vanaf de cursor omhoog (nieuwste eerst), Enter/F3 ouder, Shift+F3 nieuwer; Esc, F3 en Ctrl+C werken ook terwijl het invoerveld de focus heeft.

## 4j3. Links in de terminal

`forgeterm-terminal::links`: alleen `http://` en `https://` (andere schema's zoals `file:` of eigen protocol-handlers kunnen programma's starten; terminaluitvoer is niet te vertrouwen). Een link stopt bij spaties, quotes, `<>`, backtick, `{}|\^` en besturingstekens; leestekens aan het eind en een niet-geopende `)`/`]` vallen weg. Met Ctrl ingedrukt onderstreept de canvas de link onder de muis (kolommen rekening houdend met brede tekens) en toont een handcursor. Ctrl+klik → `is_openable` (http(s), host aanwezig, niet met `-`, geen spatie/quote/besturingsteken, ≤ 2048 tekens) → de standaardbrowser via `explorer.exe`/`xdg-open`/`open` met de link als één argument, nooit via een shell. Een geweigerde link geeft een melding.

## 4l. Sessie herstellen

`forgeterm-settings::session` (GUI-vrij, unit-getest): `session.toml` met per tab de lay-out als platte lijst in prefix-volgorde (`split = right|down` + `ratio`, of een pane met `shell` en `cwd`), plus actieve tab en viewer-bestanden. Tolerant lezen (kapotte tabs vallen weg, ratio 0.1–0.9, max 32 tabs / 16 panes per tab / 32 bestanden), atomisch schrijven (tijdelijk bestand + rename). De app schrijft alleen bij wijziging (elke 2 s vergeleken), bij Quit en als de laatste tab sluit (dan leeg). Bij het starten: lay-out via `pane_grid::State::with_configuration`; een shell die niet meer bestaat wordt de standaardshell, een map die niet meer bestaat de startmap; agent-panes worden opgeslagen als de standaardshell in hun map (nooit automatisch een agent starten). Panes van een niet-zichtbare tab starten hun shell pas bij het eerste tonen (start-op-echte-grootte).

## 4n. Icoon

Eén bron: `assets/branding/forgeterm-logo.svg`. Het venstericoon wordt bij het starten gerenderd (resvg, dezelfde versie als iced al gebruikt, 256 px, niet-gepremultiplied RGBA). Voor `forgeterm.exe` rendert `crates/forgeterm-app/build.rs` 16/24/32/48/256 px als PNG in een ICO-container en sluit die in met de resource-compiler van de Windows SDK (`embed-resource`); op andere platforms doet het build-script niets.

## 4m. Monitor

`forgeterm-system::Sampler` houdt een sysinfo-`System` vast (CPU-gebruik is een verschil tussen twee metingen; de eerste meting heeft dus geen CPU-waarde en het paneel zegt "measuring…"). Per meting: totaal CPU (alle cores = 100 %), gebruikt/totaal RAM, en per shell-pid de hele procesboom (kinderen, kleinkinderen, …; breedte-eerst) met CPU (per-proces-waarden van sysinfo gedeeld door het aantal logische cores), resident geheugen, aantal processen en programmanamen. De app meet elke 2 s op een achtergrondthread, alleen als het paneel open is en er geen meting loopt; 90 metingen historie. Grafieken: één reeks per sparkline (geen legenda), 0–100 %, kleur `CHART_CYAN` (gevalideerd), hover = kruisdraad met waarde en leeftijd. GPU wordt niet gemeten en staat er zo.

## 4k. Veilig sluiten

Quit (menu, venster-× via `exit_on_close_request: false` + `close_requests`), tab sluiten en pane sluiten gaan via `App::request_close`: van de betrokken shells (pid uit de PTY: ConPTY `child_watcher().pid()`, Unix `child().id()`) worden de kindprocessen opgezocht (`forgeterm-system`, achtergrondthread; `conhost`/`OpenConsole` genegeerd). Geen kinderen → direct sluiten; anders een bevestiging met de programmanamen per tab (Enter = sluiten, Esc/klik ernaast = annuleren). Een pane die zelf eindigt (exit 0) sluit zonder vraag.

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
