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
| `menu::tests` (6) | Elk command staat in een menu, shells per detectie, geen losse/dubbele scheidingslijnen, dropdownpositie volgt titelbreedtes, ↑↓ slaat scheidingslijnen en uitgeschakelde items over (wrap), uitgeschakeld item wordt nooit uitgevoerd | ✅ |
| `quote::tests` (9) + `shell::tests` (1) | Pad-quoting per shell (spaties, `'`, typografische quotes, `$(…)`, `~`, backslash, lege string), Git Bash- en WSL-conversie, `\\?\`-prefix, ShellKind-detectie | ✅ |
| `tests/quote_real_shells.rs` | 6 bestanden met lastige namen; elke geïnstalleerde shell leest ze via het gequote pad: PowerShell 7, Windows PowerShell, cmd, Git Bash (WSL overgeslagen). Mutatiecheck: zonder typografische-quote-regel faalt pwsh | ✅ |
| `cwd::tests` (6) + `integration::tests` (4) | OSC 7 (percent-decoding, UTF-8) en OSC 9;9 (met/zonder quotes), gesplitst over reads, laatste melding wint, andere OSC's genegeerd, misvormd/te lang/CAN genegeerd en herstel daarna, Windows/MSYS-padconversie; integratie per shell, bestaande PROMPT/PROMPT_COMMAND behouden | ✅ |
| `tests/cwd_tracking.rs` | Elke geïnstalleerde shell in echte ConPTY met integratie: startmap gemeld, `cd` naar `it's a dir` gemeld. Geslaagd: PowerShell 7, Windows PowerShell, cmd, Git Bash (eerste poging faalde voor Git Bash: `/tmp/…` → opgelost met `pwd -W`) | ✅ |
| `forgeterm-agents` (7) | PATH-zoeken (Windows: `.exe` vóór `.cmd`, `.ps1` overgeslagen; ontbrekend = None), `.cmd` → `cmd.exe /d /c`, `--version` van een echt script (lege regel overgeslagen), falend script = fout, hangend script → timeout en beëindigd; front matter; projectscan vanuit submap (git-root, rules, skills, commands in submappen, subagents, settings, MCP); zonder git = startmap | ✅ |
| `forgeterm-prompts` (8) | Front matter (BOM, CRLF, quotes, zonder afsluiting = body), variabelen in volgorde zonder dubbelingen (ongeldige `{{a b}}` / `{{}}` genegeerd, `{{{x}}}`), waarden letterlijk (geen herexpansie), leeg = ontbrekend, nooit een afsluitende newline, ingebouwde sjablonen geldig, eigen map: override, `.MD`, geen submappen/`.txt`, niet-UTF-8 gemeld, ontbrekende map = geen probleem | ✅ |
| `forgeterm-git` (12) | Porcelain v2: branch-headers, rename met origineel pad, spaties in paden, conflicten, untracked, onbekende records genegeerd, initial/detached; log-records; tegen echte git: discover vanuit submap, unborn branch, niet-ASCII + spatie, stage/unstage zonder en met HEAD, lege commit-message geweigerd, commit + log, diff unstaged/staged/untracked, fout bij onbekend pad; echt merge-conflict + rename; **repository-fsmonitor wordt niet uitgevoerd** (controle: gewone `git status` voert hem wél uit); branches parsen (ahead/behind, gone, origin/HEAD weg); tegen echte git: branch maken (lege/`-x`/`bad..name` geweigerd), lijst, wisselen, wissel geweigerd bij lokale wijziging, clone met remote branches + upstream, `--track`, history + `show`, ongeldige hash geweigerd, activiteit van vandaag; `daily_counts` (venster, buiten venster genegeerd) | ✅ |
| `forgeterm-viewer` (6) | UTF-8/BOM/UTF-16 LE+BE/binair/lossy, tabs en besturingstekens zichtbaar, afkappen, CRLF + slot-newline, kopiëren en zoeken, binair/map/te groot geweigerd, Rust-kleuren (comment/keyword/string uit het Forge-thema, spans binnen de regel), syntax per bestandstype + shebang | ✅ |
| `forgeterm-system` (4) | Echte processen: een shell met `ping`/`sleep` → programma gevonden (conhost genegeerd), wachtende shell → leeg, verdwenen pid → afwezig; `.exe` weg uit namen; sampler: eerste meting zonder CPU, tweede met CPU 0–100 %, procesboom (shell + kind, geheugen > 0, programmanaam), RAM gebruikt ≤ totaal | ✅ |
| `forgeterm-settings` (11) | Template = exact de defaults, leeg = defaults, alle waarden toegepast, 7 fouten → 7 meldingen met fallback, syntaxfout (één regel, `unreadable`), sectie geen tabel, `ensure_file` maakt één keer aan en overschrijft nooit; sessie: heen-en-terug (tabs, geneste splits, ratio's, paden met spaties/quotes, viewer-bestanden), beschadigd bestand (ontbrekend deel, onbekend split-type, lege shell, rest-items, ratio buiten bereik, active te groot), limiet panes per tab, atomisch opslaan + laden | ✅ |
| `forgeterm-explorer` (6) | Sortering, verborgen, ontbrekende map = fout, lazy uitklappen/inklappen, verouderde resultaten genegeerd, fouten + selectie | ✅ |
| `terminal_pane::tests` (3) | Tabtitels: exe-pad → shellnaam, `cmd.exe - ping …` → `ping …`, programmatitels blijven | ✅ |

Kwaliteitspoort voor iedere wijziging:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
cargo test --workspace
```

De Linux-clippy is alleen een compile-check (`rustup target add x86_64-unknown-linux-gnu`), geen validatie.

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
| G13 | (2026-11-10) Sleep-selectie + **Ctrl+C** → klembord `COPYTEST-4711`, selectie weg, geen `^C` naar de shell; **Ctrl+V** plakt zonder Enter; Ctrl+C zonder selectie → `^C` (regel afgebroken); Ctrl+V met 2 regels → bevestigingsbalk, Esc = niets uitgevoerd | Zoals beschreven | ✅ Windows |
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

### Menubalk (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| M1 | Titels File/Edit/View/Terminal/Help in topbalk, tabs en branding blijven passen | ✅ |
| M2 | Klik op File → dropdown exact onder de titel; 5 gedetecteerde shells met Ctrl+Shift+1…5 | ✅ |
| M3 | Hover over andere titel wisselt van menu; hover markeert item | ✅ |
| M4 | Niet-toepasbaar is grijs: Copy zonder selectie, Focus Pane / Toggle Zoom bij één pane | ✅ |
| M5 | Klik buiten het menu sluit het, zonder doorklik naar de terminal | ✅ |
| M6 | Terminal → Split Right voert de split uit (2 panes, 68×35) | ✅ |
| M7 | Toetsen bij open menu: → wisselt View → Terminal, ↓↓ = Split Down (grijze items overgeslagen), Esc sluit; getypt `abc` komt níet in de shell | ✅ |
| M8 | Help → ↓ Enter opent About (versie 0.1.0, maker, componenten + licenties); getypt `xyz` gaat niet naar de shell; Esc sluit | ✅ |
| M9 | File → Quit met 2 tabs: app sluit; alle 5 childprocessen (2× pwsh, 3× conhost) zijn weg | ✅ |

Gevonden en opgelost: lange items ("New Tab with Windows PowerShell") liepen buiten de dropdown → breedte 340 px.

Testvalkuil: de GUI-scripts klikken op schermcoördinaten. Ligt een ander venster bovenop, dan gaat de klik dáárheen. De helpers controleren nu eerst dat ForgeTerm het voorgrondvenster is en dat het doelpunt op ForgeTerm ligt, en breken anders af. Test niet terwijl de gebruiker de pc actief gebruikt.

### Bestandsverkenner en drag-and-drop (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| E1 | Ctrl+Shift+E toont paneel; terminal krimpt mee (140 → 108 kolommen) | ✅ |
| E2 | Chevron klapt `crates` uit; dubbelklik klapt `docs` uit | ✅ |
| E3 | Dubbelklik `Cargo.toml` in pwsh → pad achter de prompt, niet uitgevoerd | ✅ |
| E4 | `docs` → *Open tab here* → nieuwe tab met prompt in `…\docs` | ✅ |
| E5 | Git Bash-tab, *Insert path* → `/c/Erik/work/ForgeTerm/Cargo.toml` | ✅ |
| E6 | `.*` toont `.git`, `.vscode`, `.gitignore` gedimd; tooltip zichtbaar | ✅ |
| E7 | OLE-drag van `drop it's here.txt` op het venster: hint "Drop to insert the path · Quoted for Git Bash" tijdens slepen | ✅ |
| E8 | Drop in Git Bash na `cat ` → `'/c/…/drop it'\''s here.txt'`, Enter → `DROP-OK-4711` | ✅ |
| E9 | Drop in pwsh na `Get-Content ` → `'C:\…\drop it''s here.txt'`, Enter → `DROP-OK-4711` | ✅ |
| E10 | Alt+F4 met 2 tabs: alle 5 childprocessen weg | ✅ |

Gevonden en opgelost: "Erik van de Blaak Edition" brak over twee regels bij een volle tabbalk; chevrons te klein.

Testvalkuil: de OLE-drag-simulatie (`dragdrop.ps1`: eigen bronvenster, knop indrukken, cursor naar ForgeTerm, loslaten) mislukt soms door timing (`effect: None`); dan opnieuw proberen en pas Enter geven nadat het ingevoegde pad op een screenshot is gezien.

### Werkmap volgen (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| W1 | Start: statusbalk toont `C:\Erik\work\ForgeTerm`, prompt ongewijzigd (profiel geladen, geen extra uitvoer) | ✅ |
| W2 | `cd docs` → verkenner-root en statusbalk volgen direct | ✅ |
| W3 | Ctrl+Shift+T, Alt+Shift+= en Ctrl+Shift+4 (Git Bash) starten in `docs` | ✅ |
| W4 | Git Bash `cd ..` → verkenner naar ForgeTerm; Alt+1 → verkenner terug naar `docs` (map van gefocuste tab) | ✅ |
| W5 | Handmatig ↑ in verkenner blijft staan, ook na een nieuwe prompt in dezelfde map | ✅ |
| W6 | Volgen uit (`cd`-knop) + `cd C:\Erik` → verkenner blijft, statusbalk volgt; volgen aan → springt naar `C:\Erik` | ✅ |
| W7 | Restart Shell → nieuwe shell start in `C:\Erik` | ✅ |
| W8 | 4 tabs: branding blijft zichtbaar, nieuwe tab scrolt in beeld (met `+`), Alt+1 scrolt naar begin | ✅ |
| W9 | Alt+F4 met 4 tabs: alle 9 childprocessen weg | ✅ |

Gevonden en opgelost: Git Bash meldde MSYS-paden (`/tmp/…`); "Erik van de Blaak Edition" werd bij 3+ tabs afgekapt; actieve tab buiten beeld.

### Instellingen (2026-11-10, Windows 11, met FORGETERM_SETTINGS naar een testbestand)

| # | Scenario | Resultaat |
|---|---|---|
| T1 | Geen bestand: defaults, geen balk | ✅ |
| T2 | Ctrl+, maakt het bestand aan (inhoud = template) en opent het in Kladblok | ✅ |
| T3 | Live: font 18, verkenner zichtbaar, default_shell Git Bash → binnen 4 s toegepast; Ctrl+Shift+T opent Git Bash | ✅ |
| T4 | Ongeldig font, scrollback te groot, onbekende sleutel → balk met 3 meldingen, fallback; Dismiss sluit | ✅ |
| T5 | Font 20, daarna syntaxfout → font blijft 20, melding "(current settings kept)" | ✅ |
| T6 | start_directory = docs → eerste tab en verkenner starten in docs | ✅ |
| T7 | scrollback = 0 → wiel scrolt niet terug na 200 regels | ✅ |
| T8 | Niet-bestaande startmap + onbekende shell → 2 meldingen (met beschikbare shells), fallback | ✅ |
| T9 | Alt+F4: geen weesprocessen; echte `%APPDATA%\ForgeTerm` niet aangemaakt | ✅ |

Gevonden en aangepast: een syntaxfout tijdens het bewerken zette alles terug naar defaults → huidige instellingen blijven; `.toml` heeft op Windows meestal geen koppeling (alleen een keuzedialoog) → Kladblok; kopregel van de balk was misleidend bij een syntaxfout.

Let op: Kladblok in Windows 11 opent het bestand als tabblad in een al geopend Kladblok-venster van de gebruiker; sluit tijdens tests nooit Kladblok-processen.

### AI-werkplek (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| A1 | Ctrl+Shift+A: paneel met Claude Code 2.1.322 en codex-cli 0.167.0 (pad `C:\Tools\nvm\nodejs\*.cmd`), project ForgeTerm als git-root, CLAUDE.md | ✅ |
| A2 | Start Claude Code in project als tab en als split: start in de git-root (ook vanuit een submap), vertrouwensvraag van Claude Code verschijnt; Esc / dubbel Ctrl+C sluit af, tab/pane gaat dicht, geen weesprocessen; Alt+Shift+- vanuit de agent-pane opent PowerShell in de projectroot; venster sluiten met draaiende agent laat niets achter | ✅ |
| A3 | In een (vertrouwde, lege) testrepo: klik CLAUDE.md en `.claude/rules/style.md` in de Claude Code-tab → prompt `@CLAUDE.md @.claude/rules/style.md ` zonder Enter; in PowerShell → pad zonder Enter. Gevonden en opgelost: in een agent-tab toonde het paneel de context van de app-startmap (agents melden geen map) → panes vallen nu terug op hun startmap | ✅ |
| A4 | Start Codex in project: vertrouwensvraag verschijnt; Ctrl+C (2×) sluit af zonder weesprocessen van ForgeTerm. Codex laat bewust zijn eigen achtergronddienst draaien (`codex app-server --managed-daemon`, eigen ontwerp van Codex, ook buiten ForgeTerm) | ✅ |

### Prompt Studio (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| P1 | Ctrl+Shift+I: 6 ingebouwde sjablonen, "Inserts into: PowerShell 7", `cwd` automatisch ingevuld, `selection` leeg → "Fill in: selection", Insert uit | ✅ |
| P2 | Review file: `src/main.rs` getypt → voorbeeld live bijgewerkt, Insert aan; Tab springt naar het volgende veld | ✅ |
| P3 | Insert in PowerShell → tekst bij de prompt, niet uitgevoerd | ✅ |
| P4 | Plan a feature (`project` = aiproj automatisch, `feature` = Dark mode) in Claude Code → `[Pasted text #1 +4 lines]`, niet verstuurd; Ctrl+C wist, afsluiten zonder weesprocessen | ✅ |
| P5 | Open Prompts Folder → `prompts/example.md` aangemaakt, Verkenner opent die map; eigen `review-file.md` + Reload → "Mijn review" (yours) vervangt het ingebouwde, selectie blijft; Copy → klembord bevat de tekst | ✅ |
| P6 | Esc sluit; tijdens de studio getypte tekst kwam niet in de terminal | ✅ |
| S1 | Opstarten en direct typen: shell start op de echte grootte (log: `cols=140 rows=35`, was 80×24), prompt niet overschreven | ✅ |

### Git (2026-11-10, Windows 11, testrepo + ForgeTerm-repo)

| # | Scenario | Resultaat |
|---|---|---|
| G1 | Statusbalk in testrepo: `main · 4 changed`; in ForgeTerm-repo `feature/git-integration · 13 changed` (8 gewijzigd + 5 untracked, per bestand) | ✅ |
| G2 | Ctrl+Shift+G: staged (rename R), changes (M), untracked (U, ook `b c.txt` en `sub/s.txt`), commit-vak, recente commits | ✅ |
| G3 | Klik op bestand → diff in kleur (`+a2` groen, hunk cyaan); Esc sluit | ✅ |
| G4 | **+** op `b c.txt` → `git status`: `A  "b c.txt"`; **−** → weer untracked; melding in het paneel | ✅ |
| G5 | Commit-bericht typen + Commit → `git log` toont de commit, bericht leeg, lijst bijgewerkt | ✅ |
| G6 | `git add new.txt` in de terminal → paneel binnen 4 s bijgewerkt | ✅ |
| G7 | `cd ..` uit de repo → "Not a Git repository", statusbalk zonder git; terug in een repo → nieuwe status, oude actiemelding weg | ✅ |
| G8 | Scrollbalk naast de knoppen, één tooltip per element | ✅ |
| G9 | Branches in testclone: `main ↑1` current, Switch → bevestiging, git blijft op main tot Switch; Switch → `local-topic` (git bevestigt) | ✅ |
| G10 | Nieuwe branch `feature/new-ui` via veld + Enter → git staat erop, veld leeg | ✅ |
| G11 | Remote branches uitklappen: alleen `origin/remote-only` heeft Track; Track → lokale branch met upstream `origin/remote-only` | ✅ |
| G12 | Lokale wijziging in `log.txt`, wissel naar main → git weigert, melding van git in het paneel, wijziging blijft | ✅ |
| G13 | History & activity: 9 commits, staven op −20 (2), −14, −9 (3), −5, −2, −1 klopt met de testdata; tooltip "Sun 1 Nov 2026: 3 commits"; commit aanklikken → header, stat en diff | ✅ |
| G14 | Lange ref-lijst ingekort tot 3 + "+N" | unit-getest; visueel nog niet bekeken |

### Code-viewer (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| V1 | Verkenner → app.rs → View: paneel naast de terminal, Rust-kleuring, regelnummers, minimap met kader, statusregel `Rust · 2858 lines · UTF-8 · LF · 106.5 KB` | ✅ |
| V2 | Muiswiel scrolt (regels 16–47); klik in de minimap → rond regel 259 | ✅ |
| V3 | Sleep regels 246–250 + Ctrl+C → klembord bevat precies die 5 regels; Ctrl+C ging niet naar de terminal | ✅ |
| V4 | Ctrl+F `viewer_msg` → "1 of 6 lines", treffers in tekst en minimap; Enter → 2 of 6; Esc sluit | ✅ |
| V5 | Tweede tab (Python), bestand buiten ForgeTerm gewijzigd → binnen 4 s herladen met nieuwe inhoud | ✅ |
| V6 | Minimap-kader slepen tot 46 % van de hoogte → regel 1319 van 2864 (scrollbalk-gedrag, geen terugkoppeling); zoeken naar `fn load_document` → treffer 2740 in het midden; lang pad in de verkenner afgekapt bij de paneelrand | ✅ |

### Zoeken in de terminal (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| S2 | Integratietest met echte cmd/ConPTY: treffer geselecteerd, rondlopen, `1234*1000` letterlijk, smart case (hoofdletters = hoofdlettergevoelig), niet gevonden, lege zoektekst | ✅ |
| S3 | Ctrl+Shift+F, `343` (niet in de uitvoer) → "Not found" met rode rand; `spelling` → nieuwste treffer onderaan gemarkeerd | ✅ |
| S4 | 25× Enter → oudere treffers, weergave schuift het scrollback in (`↑ 19 / 131`), treffer bovenaan | ✅ |
| S5 | Ctrl+C terwijl het zoekveld de focus heeft: eerst kopieerde het níet (veld ving de toets); opgelost en hertest: klembord = treffer `77-fo` | ✅ |

### Links (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| L1 | Unit: links vinden + leestekens/haakjes, geen file/ftp/javascript, niet aan een woord vastgeplakt, kolomposities, `is_openable` weigert spaties/quotes/ESC/te lang/`-host` | ✅ |
| L2 | `https://example.com/forgeterm-test` in de uitvoer, Ctrl ingedrukt + muis erover → onderstreept; Ctrl los → weg | ✅ |
| L3 | Ctrl+klik opent de browser | ⬜ bewust niet in de GUI uitgevoerd (zou de browser van de gebruiker openen en een externe site bezoeken); pad = `is_openable` (unit-getest) + `explorer.exe <url>` |

### Monitor en icoon (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| M1 | `ping` in de tab, Ctrl+Shift+M → per terminal "2 processes · PING, 94 MB"; Windows zelf (WMI): pwsh + PING.EXE, 94 MB working set | ✅ identiek |
| M2 | Systeemgeheugen 11.5 / 15.7 GB (73 %); Windows: 11.5 / 15.7 GB | ✅ identiek |
| M3 | GPU staat er als "not measured" (niet verzonnen) | ✅ |
| M4 | Hover over de geheugen-sparkline → kruisdraad + "74 % · 16 s ago" (eerst niet: canvas tekende niet opnieuw bij muisbeweging; opgelost) | ✅ |
| M5 | Na `ping`: "1 process · waiting at the prompt"; ForgeTerm hard beëindigd → geen achtergebleven ping | ✅ |
| I1 | Venstericoon (titelbalk) = logo; `forgeterm.exe` bevat het icoon (ExtractAssociatedIcon toont het F-logo) | ✅ |

### Kitty-toetsenbordprotocol (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| KB1 | Unit: vlag 1 (Shift+Enter `CSI 13;2u`, Enter `\r`, Esc `CSI 27u`, Ctrl+C `CSI 99;5u`, Alt+a, Ctrl+Shift+A `CSI 97;6u`, tekst blijft tekst, pijlen legacy), vlag 8+16 (tekst als `CSI 97;1;97u`, Enter `CSI 13u`), zonder vlaggen = legacy | ✅ |
| KB2 | Claude Code starten → log: vlaggen 5; afsluiten → 0 | ✅ |
| KB3 | Byte-echo (Node, zet vlag 1 aan) in ForgeTerm via ConPTY: Shift+Enter `1b 5b 31 33 3b 32 75`, Enter `0d`, Ctrl+A `CSI 97;5u`, Esc `CSI 27u`, `a` — ConPTY laat de reeksen ongewijzigd door | ✅ |
| KB4 | Claude Code: `/zz`, Shift+Enter, `yy` → twee regels in de invoer, niets verstuurd; Backspace werkt; Ctrl+C wist; 2× Ctrl+C sluit af | ✅ |

### Linux: WSL Ubuntu 24.04 (basis van Linux Mint 22), 2026-11-10

Rust 1.99.1 per gebruiker (rustup, geen sudo); verse clone in het Linux-bestandssysteem. Geen `-dev`-pakketten nodig: iced laadt X11/Wayland/xkbcommon via dlopen, alleen gcc als linker.

| # | Scenario | Resultaat |
|---|---|---|
| LX1 | `cargo fmt --check`, `cargo build`, `cargo clippy --all-targets -D warnings` op Linux | ✅ |
| LX2 | `cargo test --workspace --no-fail-fast`: 142 tests, incl. echte PTY met `/bin/sh`, cwd-tracking en quoting in echte shells. Eerste run vond 2 tests met Windows-verwachtingen (OSC 9;9 met `C:\\…`, `//host/share`); platformbewust gemaakt | ✅ |
| LX3 | GUI onder WSLg (Wayland, geen GPU-doorvoer voor Mesa: wgpu valt terug op GLES via EGL): venster, menu, tabs, bash-prompt in kleur, statusbalk met `bash`, map (bash-integratie) en git-status `master ↑2 · clean` (klopt met de clone). Vastgelegd met de testhaak `FORGETERM_SCREENSHOT` (het WSLg-venster zelf is van buitenaf niet te fotograferen: zwart of doorzichtig) | ✅ |
| LX4 | Invoer via het WSLg-venster: `touch /tmp/ft_input_ok; cd crates; ls --color=auto` + Enter → bestand bestaat in Linux, gekleurde uitvoer, statusbalk volgt direct naar `~/forgeterm-linux-test/crates` | ✅ |
| LX5 | Echte Linux Mint-desktop (Cinnamon, X11) | ⬜ niet getest — WSL is dezelfde Ubuntu-basis maar een andere desktop/compositor |

### Pakketten (2026-11-10)

| # | Scenario | Resultaat |
|---|---|---|
| PK1 | `scripts/package-linux.sh` in WSL Ubuntu 24.04 → `forgeterm_0.1.0-1_amd64.deb`; `dpkg-deb -I/-c`: binary 755 in `/usr/bin`, `.desktop`, SVG-icoon in hicolor, README/licenties/THIRD-PARTY/copyright in `/usr/share/doc/forgeterm`; Depends `libc6 (>= 2.39)` (Mint 22 = 2.39) + X11/Wayland/xkbcommon/EGL | ✅ |
| PK2 | `apt-get install --simulate ./forgeterm_…deb`: afhankelijkheden lossen op (alleen `libxkbcommon-x11-0` + `libxcb-xkb1` extra in kale WSL) | ✅ |
| PK3 | Uitgepakt pakket (`dpkg -x`) starten met `WAYLAND_DEBUG=1`: `xdg_toplevel.set_app_id("forgeterm")` (= naam van het `.desktop`-bestand); daarvoor was het leeg | ✅ |
| PK4 | Echt installeren met `sudo apt install` op een Mint-desktop (menu-item, icoon in taakbalk) | ⬜ niet getest |
| PK5 | `scripts/package-windows.ps1` zonder NSIS: zip gemaakt, installer netjes overgeslagen met melding | ✅ |
| PK6 | Windows-installer bouwen en installeren/upgraden/verwijderen (instellingen blijven) | ⬜ NSIS niet aanwezig |

### Beveiliging en sneltoetsen (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| X1 | Unit: OSC 9;9 / OSC 7 met `\\\\host\\share`, `//host/share`, `file://x//host/share`, `\\\\?\\UNC\\…`, `\\\\.\\pipe\\…`, relatief → genegeerd | ✅ |
| X2 | Echte git: repository met `filter.evil.clean` → `repo_programs` meldt het; controle: gewone racy `git status` en `git diff` vóérden het filter uit | ✅ |
| X3 | GUI: `cd` naar zo'n repo → statusbalk "git paused (repository runs programs)", paneel toont het programma + *Trust this repository*; filter niet uitgevoerd; na Trust gewone status | ✅ |
| X4 | Unit: plakken zonder brackets laat Ctrl+O/ESC/DEL/C1 weg, tab en regeleinden blijven; bracketed verwijdert ook U+009B | ✅ |
| X5 | GUI: script stuurt OSC 52 → klembord gezet én melding "PowerShell 7 (tab 1) copied 10 characters to the clipboard." | ✅ |
| X6 | Unit: agentpad `C:\\a&b\\claude.cmd` / met `%…%` geweigerd, `.exe` met `&` toegestaan | ✅ |
| K1 | `[keys]` `"F6" = "keyboard_shortcuts"`, `"Ctrl+Shift+G" = "none"`, `"Ctrl+Alt+G" = "toggle_git_panel"` → F6 opent het overzicht, Git-paneel toont alleen Ctrl+Alt+G en opent daarmee | ✅ |
| K2 | Unit: toetstekst parsen (incl. `Ctrl++`, F1–F12), commandonamen uniek en omkeerbaar, toevoegen/vervangen/verwijderen, eerlijke melding bij Ctrl+letter, onbekende namen gemeld | ✅ |

### Release-build (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| B1 | `cargo build --release` (lto thin): 60 s, `forgeterm.exe` 18,4 MB | ✅ |
| B2 | Release gestart in de ForgeTerm-repo, Git-paneel + AI-paneel open (git status elke 4 s, log, branches, `claude.cmd`/`codex.cmd --version`); zichtbare console-/Terminal-vensters elke 0,1 s geteld gedurende 45 s: maximum = beginwaarde (1) → geen enkel flitsend consolevenster | ✅ |
| B3 | `scripts/package-windows.ps1` → zip (8,2 MB) met exe, README, OFL, THIRD-PARTY.txt (248 crates, elk met licentie; geen copyleft-verplichting: `self_cell` is Apache-2.0 OR GPL-2.0) + SHA-256 | ✅ |

### Sessie herstellen (2026-11-10, Windows 11, FORGETERM_SETTINGS naar de testmap)

| # | Scenario | Resultaat |
|---|---|---|
| R1 | Split rechts + `cd ..` rechts, reload.py in de viewer, tweede tab, Alt+F4 → `session.toml` bevat precies die indeling (ratio 0.5, beide mappen, actieve tab 1, het bestand) | ✅ |
| R2 | Opnieuw starten vanuit een andere map → tab 2 actief in scratchpad, viewer met reload.py; Alt+1 → tab 1 met de split, links viewtest, rechts scratchpad | ✅ |

### Veilig sluiten (2026-11-10, Windows 11)

| # | Scenario | Resultaat |
|---|---|---|
| Q1 | Alleen een wachtende PowerShell: Alt+F4 sluit direct, zonder vraag | ✅ |
| Q2 | `ping -n 120` loopt: Ctrl+Shift+W → "Close this pane? · PING in PowerShell 7 (tab 1)"; Esc → ping loopt door; Alt+F4 → "Quit ForgeTerm?"; Enter → venster dicht en ping beëindigd | ✅ |

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

**Hermeting 2026-11-10 (avond, release met viewer, git, monitor, syntect, sysinfo):** idle met één tab **163 MB** working set (179 MB private) — binnen het budget van 175 MB; met de viewer (app.rs, 3.697 regels, gekleurd) en de Monitor open 184 MB. Release-binary nu 18,4 MB (was 15 MB; syntect-syntaxen en sysinfo).

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
