# ForgeTerm — prestaties

Meten eerst, dan pas verbeteren (P2 in FORGETERM_IMPLEMENTATION_PLAN). Dit document legt de metingen vast en de beslissingen die eruit volgden.

## Hoe meten

```
cargo bench -p forgeterm-terminal     # emulator, snapshot, link-scan, terminal-zoeken
cargo bench -p forgeterm-viewer       # viewer-zoeken
```

- De benches gebruiken `forgeterm_terminal::Headless`: dezelfde emulator en dezelfde snapshotcode als een echte sessie, maar zonder PTY. Ze zijn dus reproduceerbaar en hebben geen shell nodig.
- `criterion` is alleen een dev-dependency; de app zelf verandert er niet door.
- Machine: Windows 11, i7 (RaptorLake, laptop), release-profiel. Absolute getallen verschillen per machine; vergelijk alleen metingen van dezelfde machine.

## Microbenchmarks (2026-11-16)

| Bench | Scenario | Tijd |
|---|---|---|
| `emulator/feed_1mb_build_output_120x40` | 1 MB compiler-achtige uitvoer (kleuren, paden, URL's) door de emulator | 11,0 ms (~91 MiB/s) |
| `emulator/feed_tui_frame_120x40` | één volledig TUI-frame in truecolor (zoals Claude Code), synchronized update | 58 µs |
| `snapshot/tui/120x40` | viewport naar snapshot | 39 µs |
| `snapshot/tui/240x70` | idem, groot venster | 136 µs |
| `scan/links_and_paths_screen_120x40` | URL- en padherkenning over een heel scherm (40 regels) | 234 µs |
| `search/scrollback_10k_lines_miss` | terminal-zoeken zonder treffer, 10.000 regels scrollback | **8,8 ms** |
| `search/scrollback_10k_lines_hit` | idem, treffer vlakbij | 25 µs |
| `viewer_find_7_5mb/miss` / `hit_every_line` | viewer-zoeken in een bestand van 7,5 MB | 2,6 / 3,1 ms |
| (los gemeten) thread starten + resultaat | één `background()`-taak | 50 µs |

## Wat de metingen zeggen, en wat ermee gedaan is

De audit (§6) schatte de knelpunten zonder te meten. Dit klopte wel en dit niet:

- **P1, snapshot per wakeup.** Een snapshot kost 39 µs, dus dat is niet het probleem. De echte kosten zitten in het opnieuw tekenen van het canvas na elke snapshot, ook als het scherm gelijk bleef. TUI's tekenen vaak hetzelfde frame opnieuw.
  - **Gedaan:** `Session::refresh` leest snapshot en modi onder één lock (eerst twee) en geeft alleen een nieuwe snapshot terug als die verschilt. De pane wist zijn canvas-cache alleen dan.
  - De test `redrawing_the_same_frame_gives_an_equal_snapshot` bewaakt de aanname.
  - Een buffer hergebruiken is niet gedaan: de allocatie zit in die 39 µs en valt weg tegen een frame van 16 ms.
- **P3, terminal-zoeken.** Dit klopte. Een zoekopdracht zonder treffer scant de hele scrollback onder de terminal-lock op de UI-thread: 8,8 ms per 10.000 regels, dus rond de 90 ms bij het maximum van 100.000. Dat werd per toetsaanslag gedaan.
  - **Gedaan:** er wordt pas gezocht na 150 ms zonder typen (`TERM_FIND_DEBOUNCE`); Enter zoekt meteen.
- **P3, viewer-zoeken.** 3 ms bij 7,5 MB. **Niet aangepast**; de kopie in kleine letters per toets is meetbaar maar onschuldig.
- **Link-scan.** Die draait alleen voor één regel onder de muis met Ctrl (ongeveer 6 µs). **Niets aan gedaan.**
- **P5, timers.** `GitTick` (een git-proces elke 4 s) en `SettingsTick` (elke 2 s) liepen altijd.
  - **Gedaan:** ze lopen alleen als het venster focus heeft, en bij terugkeer wordt één keer ingehaald.
  - Eén langlevende worker in plaats van een thread per taak is **niet gedaan**: 50 µs per taak bij ongeveer 0,5 taak per seconde is ongeveer 0,003 % CPU.

## Eind-tot-eind in de app (2026-11-16)

CPU-tijd van het ForgeTerm-proces (`TotalProcessorTime`), release-builds, dezelfde instellingen, `master` (52bc762) tegen de P2-branch, om en om gedraaid.

- **Scripts:**
  - de scenario's zelf: `same.ps1` (60 fps hetzelfde frame), `changing.ps1` (elk frame anders) en `onemb.ps1` (1 MB uitvoer), in de shell van de test-instantie;
  - de runners: `perf.ps1` en `idle.ps1` (scratchpad).
- **Opzet:**
  - de test-instantie draait met een eigen `FORGETERM_SETTINGS`;
  - elke run wacht tot de gebruiker 20 s niets heeft aangeraakt;
  - aan het eind wordt het testproces hard beëindigd.

### Actieve TUI (12 s)

| Scenario | master | P2 |
|---|---|---|
| TUI tekent 10 s lang hetzelfde frame (~60 fps) | 781 / 797 / 969 ms | 1.047 / 953 / 859 ms |
| Controle: elk frame anders | 906 ms | 812 ms |
| 1 MB uitvoer | 125 ms | 156 ms |

**Geen meetbaar verschil**: de spreiding tussen runs (±15 %) is groter dan elk verschil. Uitgezocht waarom, met tijdelijke tracing:

- **Refreshes:** van de 508 refreshes in 12 s werden er 155 (30 %) overgeslagen als "zelfde scherm". De andere kwamen binnen als stukken van een frame. ConPTY geeft het VT van het programma niet 1-op-1 door, maar rendert zelf en stuurt alleen verschillen met eigen cursorbewegingen. Identieke frames zijn dus al grotendeels gefilterd voordat ze bij ForgeTerm komen.
- **Waar de tijd zit,** van ~860 ms CPU in totaal:

  | Onderdeel | Totaal | Mediaan per aanroep |
  |---|---|---|
  | `handle` (berichten, incl. snapshot) | 75 ms | 106 µs |
  | `draw_snapshot` | 14 ms | 31 µs |
  | `view()` | 12,5 ms | 20 µs |

  ForgeTerms eigen logica is dus ~100 ms. De rest (~1,5 ms CPU per gerenderd beeld) zit in iced/wgpu en de grafische driver.
- **Gevolg voor de audit:**
  - P4 ("`view()` bij elk bericht is duur") klopt niet: 20 µs.
  - De enige grote hefboom zou minder beelden renderen zijn, en dat is bij een TUI niet wenselijk.

### Op de achtergrond (30 s, na een echte gebruikersactie)

| Na | master | P2 |
|---|---|---|
| Alt+Tab naar een ander venster | 219 ms | **0 ms** |
| Minimaliseren (Win+↓) | 188 ms | **47 ms** |

De idle-timers (git-status elke 4 s, instellingen elke 2 s) lopen alleen nog met focus; bij terugkeer wordt één keer ingehaald. Met de tijdelijke trace gecontroleerd: na `Unfocused` start er geen `git status` meer.

**Meetvalkuil:**
- Focus wegnemen met `SetForegroundWindow`/`ShowWindow` vanuit een ander proces levert in winit **geen** `Focused(false)` op. Daardoor leek het eerst alsof de pauze niet werkte.
- Een echte Alt+Tab of Win+↓ (`keybd_event`) wel.
- Test focusgedrag dus altijd met echte invoer.

### Beslissingen

- **Gehouden:**
  - de refresh onder één lock met het overslaan van gelijke snapshots: juist, goedkoop en getest, al is de winst onder ConPTY klein;
  - de debounce van het zoeken in de terminal;
  - de pauze van de timers zonder focus.
- **Niet gedaan,** omdat de metingen dat niet rechtvaardigen:
  - een worker-pool;
  - buffers hergebruiken;
  - de viewer-zoekfunctie;
  - `view()` cachen.
- **Om zelf na te meten:** `RUST_LOG=forgeterm::terminal_pane=trace` toont per refresh `changed=true/false`.
