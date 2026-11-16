# ForgeTerm — Architectuuraudit

Datum: 2026-11-16 · Basis: `master` @ `b4475f1` (v0.1.1) · Uitgevoerd op Windows 11 (26300).

**Hoe dit tot stand kwam.** Drie onafhankelijke, alleen-lezende reviews (security, performance/threading, codekwaliteit/UX/documentatie) plus eigen controle. Per bevinding staat hoe ze geverifieerd is:

- **[R]** gereproduceerd met echte programma's;
- **[C]** gecontroleerd door de code te lezen (volledig codepad);
- **[Z]** zelf nagekeken in deze sessie.

Er zijn **geen benchmarks**. Uitspraken over performance zijn inschattingen op basis van code, geen metingen.

**Uitgangsmeting (baseline) vóór wijzigingen:**

| Controle | Resultaat |
|---|---|
| `cargo fmt --check` | groen |
| `cargo clippy --workspace --all-targets -D warnings` | groen |
| `cargo test --workspace` | 177 tests groen (incl. echte PTY- en git-tests) |

CI (`.github/workflows/ci.yml`) draait fmt, clippy en tests op `windows-latest` en `ubuntu-24.04`.

---

## 1. Bestaande functionaliteit

| Gebied | Wat er is (en getest) |
|---|---|
| Terminal | alacritty_terminal 0.26 (VT/xterm, truecolor, alt screen, scrollback, OSC 52 alleen schrijven), ConPTY/Unix-PTY, kitty keyboard protocol (vlag 1/8/16), tabs, splits, zoom, zoeken, links (alleen http(s)), bestandsverwijzingen → viewer |
| Shell-integratie | werkmap (OSC 7 / 9;9), commandomarkeringen (OSC 133/633), programmastatus (OSC 7501), zonder profielwijzigingen; pwsh, Windows PowerShell, cmd, Git Bash, Linux-bash |
| Werkplek | command registry + palette + menubalk + `[keys]`, verkenner (lazy, toetsenbord, auto-refresh), sessieherstel (`session.toml`), veilig sluiten |
| AI | detectie Claude Code / Codex (+ versie), starten in projectroot, projectcontext (CLAUDE.md, AGENTS.md, skills, …), `@pad` invoegen, laatste uitvoer → agent, Prompt Studio (sjablonen, variabelen, nooit Enter) |
| Git | status/diff/stage/unstage/commit, branches, history + activiteitsgrafiek, trust-pauze bij repo-config met programma's |
| Viewer | syntect-kleuring, minimap, zoeken, live herladen, besturingstekens zichtbaar |
| Monitor | CPU/RAM systeem en per terminal-procesboom (sysinfo); GPU eerlijk "not measured" |
| Release | Windows-zip + NSIS-installer, `.deb` (Mint 22 / Ubuntu 24.04), icoon, licentielijst |

## 2. Architectuur en crate-afhankelijkheden

```text
forgeterm-app (iced 0.14, binary)
 ├─ forgeterm-terminal   engine: Session, input, palet, snapshot, shell, OSC-scanners   (GUI-vrij)
 ├─ forgeterm-agents     detectie, launch, projectcontext, mission-statusmodel          (GUI-vrij, geen deps)
 ├─ forgeterm-prompts    sjablonen + render                                             (GUI-vrij)
 ├─ forgeterm-git        git-CLI (porcelain v2), refs, history, HEAD-lezer              (GUI-vrij, chrono)
 ├─ forgeterm-viewer     laden/decoderen/kleuren/zoeken                                 (GUI-vrij, syntect)
 ├─ forgeterm-explorer   bestandsboom                                                   (GUI-vrij)
 ├─ forgeterm-system     procesboom + CPU/RAM (sysinfo)                                 (GUI-vrij)
 └─ forgeterm-settings   settings.toml + session.toml                                   (GUI-vrij)
```

- **Event loop:** Elm-architectuur (iced). Achtergrondwerk via `background()`: één OS-thread per taak, met als resultaat een `Message`.
- **PTY-I/O:** draait op alacritty's eigen thread. Wakeups worden gecoalesced met een `AtomicBool`.

## 3. Technische sterke punten

- **Strikte scheiding engine/GUI.** Alle parse-, encodeer- en beveiligingslogica zit in GUI-vrije crates met unit tests.
- **Eén plek voor acties** (`CommandId` → `App::run_command`), met conflict- en menutests.
- **Security-bewust ontwerp.** Plakken nooit met Enter, ESC gestript in bracketed paste, OSC 52 lezen geblokkeerd, UNC-paden uit terminaluitvoer geweigerd, git met vaste veilige opties en een trust-pauze.
- **Langzaam werk zit niet op de UI-thread.** git, verkenner, viewer, highlighting, sampling en sluitcontrole draaien elders.
- **Begrensd geheugen** bijna overal: scrollback, history, listings, diffs, viewerbestanden.
- **Eerlijke documentatie** van beperkingen (ROADMAP "Bekende beperkingen").

## 4. Concrete tekortkomingen

| # | Bevinding | Plaats | Verif. |
|---|---|---|---|
| T1 ✅ opgelost 2026-11-16 (`Message::WorkerPanicked`: logt, zet alle busy-vlaggen terug, melding; de instellingenbanner blijft) | Een gecrashte achtergrondtaak wordt `Message::DismissSettingsProblems`. Dat wist de instellingenbanner, en busy-vlaggen (`git.refreshing`, `git.action_running`, `monitor.busy`, `explorer_checking`) blijven dan voor altijd staan. Git, monitor of verkenner stoppen stil tot je herstart. | `app.rs` `background()` | [Z] |
| T2 ✅ opgelost 2026-11-16 (één veld `side: Option<SidePanel>`: twee zijpanelen tegelijk kan per type niet meer; `load_settings` en `ExplorerGoToResult` gebruiken het ook) | De zijpanelen zijn 4 losse booleans, op 5 plaatsen met de hand exclusief gemaakt. `load_settings` en `ExplorerGoToResult` slaan dat over, waardoor git en verkenner tegelijk "zichtbaar" kunnen zijn (git haalt dan onzichtbaar zijn log op). | `app.rs` `load_settings`, `ExplorerGoToResult` | [Z] |
| T3 ✅ opgelost 2026-11-16 (`Keymap::hint`/`range_hint`: hints volgen `[keys]`, een verwijderde toets verdwijnt uit de tekst; getest) | Sneltoetshints staan hardcoded ("Ctrl+Shift+W", "Ctrl+Shift+Enter", "Alt+1…9") en kloppen niet meer na `[keys]`. | `app.rs` view-functies, shortcuts-overlay | [C] |
| T4 ✅ opgelost 2026-11-16 (9 nieuwe commands; grens command/message in ARCHITECTURE §commands; per-item-acties en dialoogknoppen blijven bewust messages) | Ongeveer 30 `Message`-varianten voeren gebruikersacties direct uit (git stage/commit, prompt insert, tab select/close). Die staan dus niet in de palette en hebben geen sneltoets, in strijd met CLAUDE.md. | `app.rs` `handle` | [C] |
| T5 ✅ opgelost 2026-11-16 (een niet-toepasselijke sneltoets doet niets en gaat ook niet naar het programma) | Sneltoetsen controleren `is_applicable` niet. | `app.rs` `app_shortcut` | [C] |
| T6 ✅ opgelost 2026-11-16 (melding met pad en gevolg, één keer per foutsoort; ook de logregel) | Een mislukte `session.toml`-opslag is alleen een log-waarschuwing. De gebruiker merkt pas bij herstart dat herstel niet werkt. | `app.rs` `save_session` | [C] |
| T7 ✅ opgelost 2026-11-16 (naam wordt pas na succes gewist) | `GitAction::Create` wist de getypte branchnaam vóór het resultaat bekend is. Bij een fout ben je de invoer kwijt. | `app.rs` `git_action` | [C] |
| T8 ✅ opgelost 2026-11-16 (eerst `is_local_path` + `resolves_locally`, pas dan `is_dir`) | `start_directory.is_dir()` op de UI-thread, zonder `is_local_path`. Een UNC-pad in de instellingen kan de UI blokkeren. | `app.rs` `load_settings` | [C] |

## 5. Securityrisico's

| # | Ernst | Bevinding | Verif. |
|---|---|---|---|
| S1 | **Hoog** — ✅ opgelost 2026-11-16 | **Submodules omzeilen de git-trustpauze.** `repo_programs` leest alleen de config van het superproject, maar `git status` (zonder `--ignore-submodules`) recurseert in submodules en voert daar `filter.*.clean` uit. Gereproduceerd: het markerbestand werd 2× geschreven. Oplossing: `--ignore-submodules=all` + `-c diff.ignoreSubmodules=all`, of trustcontrole per submodule. | [R] |
| S2 | Middel (Win) — ✅ opgelost 2026-11-16 (`forgeterm-fs::resolves_locally`) | **Symlinks omzeilen de UNC-weigering.** `is_local_path` controleert alleen de tekst van het pad; `fs::metadata` (verkenner), `is_file` (bestandslinks) en `is_dir` (OSC 7) volgen reparse points. Op deze machine: een symlink naar `\\127.0.0.1\…` leidde tot een SMB-verbinding (NTLM-lek), en Developer Mode stond aan. Oplossing: `symlink_metadata` + `read_link` + `is_local_path` op het doel. | [R] primitief, [C] keten |
| S3 | Middel (Linux) — ✅ opgelost 2026-11-16 | **Viewer laadt apparaatbestanden.** Een repo met `x -> /dev/zero` laat `fs::read` groeien tot de app afbreekt; een FIFO laat de thread eeuwig hangen. Oplossing: `meta.is_file()` + `File::open(..).take(MAX+1)`. | [C] |
| S4 | Laag | Plakbevestiging gebruikt gecachete modi, het encoderen live modi. Een programma dat net bracketed paste uitzet, krijgt meerregelige tekst met CR zonder bevestiging. Oplossing: beslissen en encoderen onder één lock in `Session`. | [C] |
| S5 | Laag | git-stdout onbegrensd (`read_to_end`). Een diff van een enorm bestand komt eerst volledig in het geheugen. Oplossing: `take()`. | [C] |
| S6 | Open | `git config --includes` vóór trust kan een UNC-`include.path` aanraken. Niet waargenomen; één test waard. | — |

Gecontroleerd en in orde: OSC-scanners (limieten), commandomarkeringen, OSC 7501, links, git-argumenten, agentstart (`cmd /d`, metatekens), paste-encodering, logging (geen terminalinhoud of env), sessiebestand, begrensde reads.

## 6. Performance

| # | Bevinding | Impact (niet gemeten) |
|---|---|---|
| P1 | Elke output-wakeup maakt een volledige viewport-snapshot (nieuwe `Vec`), wist de canvas-cache en tekent alles opnieuw, ook als de inhoud gelijk is. Daarna nog een tweede lock voor `modes()`. | Grootste CPU-post bij TUI's en streaming output |
| P2 | Snapshots zijn niet aan frames gekoppeld. Elke snapshot neemt de Term-lock, waardoor de parser wacht. | Mogelijk meer snapshots dan frames |
| P3 | Zoeken in de terminal bouwt per toetsaanslag een regex en scant tot 100k regels onder de lock op de UI-thread. Viewer-zoeken maakt per toets een lowercase-kopie van een document van tot 8 MB. | Haperend typen bij grote scrollback/bestanden |
| P4 | `view()` bouwt grote boomstructuren bij elk bericht, ook bij elke output-wakeup: diff-overlay tot 5000 `text`-widgets, de hele verkennerboom, het git-paneel. | Merkbaar bij streaming output met paneel open |
| P5 | Idle-timers: `SettingsTick` 2 s (altijd), `GitTick` 4 s (altijd, 2 git-processen), `ExplorerTick` 2 s. Plus één OS-thread per `background()`. | ~1–2 berichten/s en 0,5 git-proces/s in rust |
| P6 | Viewer-tabs onbegrensd (tot 8 MB + spans per tab). | Geheugen bij veel open bestanden |

Goed al: wakeup-coalescing, verborgen panes kosten bijna niets, viewer cachet per tab en tekent alleen zichtbare regels, geen cursor-knippertimer.

## 7. UX

- **Typografie en vormen zonder tokens.** Groottes 9–18 en radii 0–10 staan verspreid in de code; DESIGN_SYSTEM noemt alleen 15/13/12 en radius 6.
- **Toetsenbord.** Het git-paneel en de AI-contextlijst zijn niet met het toetsenbord te bedienen.
- **Tooltips ontbreken** op "+", "×" en de git-samenvatting.
- **Contrast.** `DISABLED` (3,0:1) wordt ook voor gewone informatieve tekst gebruikt.
- **Goed:** lege, laad- en foutstaten zijn bijna overal netjes. Uitzondering: een mislukte sessie-opslag (T6).

## 8. Technische schuld

- ✅ **Opgesplitst 2026-11-16** in `app/` (18 bestanden, 39–701 regels; per onderwerp een `impl App`-blok; alleen verplaatst, aangetoond met een regel-voor-regel-vergelijking en dezelfde 222 tests). Geneste messages zijn nog niet gedaan. Oorspronkelijk: **`app.rs` is 4300+ regels**: 95 `Message`-varianten en 45 velden in één `impl`. Voorstel:
  - `app/{message,keyboard,git,explorer,prompts,ai,session,close,view}.rs` met eigen `impl App`-blokken;
  - geneste messages (`Git(GitMsg)` enz., zoals `Viewer(ViewerMsg)` al doet);
  - `run_command` blijft de enige dispatch.
- **Duplicatie:**
  - 3 open-in-OS-functies (elk met een macOS-tak die nooit gebouwd wordt);
  - de clipboard-CRLF-conversie, de home-dir-lookup, de 22 px rijhoogte (3×) en de modal-scrim (6×);
  - `viewer_view::to_color` dupliceert `theme::to_color`.
- **Achterhaalde markeringen:** `#![allow(dead_code)]` in `theme.rs` en een verkeerd geplaatst sectiecommentaar.
- ✅ **Docs-drift** weggewerkt 2026-11-16 (kop, §3-boom, §7 platform/Linux, CI-sectie, testaantallen). Oorspronkelijk:
  - ARCHITECTURE-kop zegt nog "Fase 1", §7 "Linux nog niet gevalideerd", §3-boom mist ±15 bestanden, en er staat een verwijzing naar `links` (moet `url` zijn);
  - testaantallen in ROADMAP/TEST_PLAN (142 vs. 177);
  - CI ontbreekt in de docs.
- **Testgaten:** `app.rs` (0 tests), `keys.rs`, muis→cel-mapping, `glyphs.rs`, `git::run` (timeout), `snapshot.rs`.

## 9. Aanbevolen uitbreidingen — en wat botst met de projectregels

De masterprompt van 2026-11-16 overlapt grotendeels met de oorspronkelijke 7-fasen-spec. Wat er al is, staat in §1. Een aantal eisen vraagt een besluit, omdat ze botsen met CLAUDE.md:

| Eis | Botsing | Voorstel |
|---|---|---|
| Tokens en kosten per agent | Geen gedocumenteerde bron. Sessiebestanden van agents lezen is geen gedocumenteerde interface. | Pas tonen als de agent het zelf meldt of er een officiële export is; tot dan "niet beschikbaar". |
| AI Comparison Mode: "dezelfde opdracht aan twee agents geven" | ForgeTerm stuurt nooit zelf prompts; invoegen gebeurt zonder Enter. | Opdracht in beide terminals **invoegen** zonder Enter; de gebruiker drukt zelf Enter. Vergelijken (diff, tests, clippy) gebeurt op de worktrees. |
| "Huidige taak" en "Waiting for user" | Alleen betrouwbaar als de agent het meldt. | Mission Control toont de venstertitel en OSC 7501, anders alleen feiten. Geïmplementeerd. |
| Sessie hervatten | Moet een gedocumenteerde CLI-vlag zijn. | `claude --continue` / `--resume` en `codex resume` eerst tegen de actuele docs verifiëren, dan pas aanbieden. |
| SSH/SFTP/Docker | Grote nieuwe oppervlakken (crypto, credentials). | Fase 4, eigen ontwerp + securityreview; geen eigen crypto, OS-keychain/ssh-agent. |
| GPU-metingen | Alleen via NVML, ADLX of sysfs. | Providerarchitectuur in `forgeterm-system`, met "niet beschikbaar" als standaard. |

## 10. Implementatievolgorde

Zie `docs/FORGETERM_IMPLEMENTATION_PLAN.md`. Kort:

1. Securityfixes S1–S3 en de stille-fout-bugs T1/T2 (P0).
2. Mission Control (M1, gebouwd op `feature/mission-control`).
3. Git Control Center als dashboard + worktrees (M2).
4. Performance: eerst meten (benchmarks en baseline), dan P1–P3.
5. Opsplitsen van `app.rs` vóór grotere nieuwe UI.
6. Thema's en design tokens.
7. Parallel AI (worktree per agent, vergelijken).
8. Fase 4: SSH, Docker, GPU.
