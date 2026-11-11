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
| Bestandsverkenner | **Ctrl+Shift+E**: verborgen → tonen met toetsenbord, zichtbaar → toetsenbord erheen, al actief → verbergen. In de boom: ↑↓ PgUp/PgDn Home/End, → uitklappen / naar eerste item, ← inklappen / naar oudermap, Enter = map open/dicht of bestand in de viewer (die krijgt dan het toetsenbord), Esc = terug naar de terminal (gewoon typen ook: de toets gaat meteen naar de terminal). Ververst vanzelf als er bestanden bijkomen of verdwijnen |
| Andere map in de verkenner | knop **…** of View → Go to Folder…: pad typen of plakken (`~` = thuismap, relatief = vanaf de huidige map), Enter |
| Instellingen openen (`settings.toml`, wordt live toegepast) | **Ctrl+,** of File → Open Settings File |
| AI-werkplek (Claude Code / Codex starten, projectcontext) | **Ctrl+Shift+A** of menu AI |
| Prompt Studio (sjabloon kiezen, variabelen invullen, invoegen zonder Enter) | **Ctrl+Shift+I** of AI → Prompt Studio |
| Prompt Studio naast de terminal (of weer zwevend) | knop *Dock beside terminal* / *Float* in de studio, of AI → Prompt Studio Beside Terminal / Floating |
| Monitor: CPU en geheugen van het systeem en per terminal (echte metingen) | **Ctrl+Shift+M** of View → Toggle Monitor |
| Code-viewer (bestanden bekijken met kleuring, minimap, zoeken) | **Ctrl+Shift+O**, View in de verkenner, of *Open in viewer* in een Git-diff |
| Git-paneel (status, diff, stage/unstage, commit, recente commits) | **Ctrl+Shift+G**, menu Git, of klik op de branch in de statusbalk |
| Pad in de terminal zetten (gequote voor de shell, níet uitgevoerd) | dubbelklik op bestand, knop *Insert path*, of sleep een bestand uit Windows Verkenner op het venster |
| Link in de terminal openen | **Ctrl+klik** (Ctrl ingedrukt = link onderstreept); alleen `http`/`https` |
| Bestand uit een foutmelding openen | **Ctrl+klik** op `src/main.rs:42:5`, `Program.cs(12,5)`, `File "app.py", line 3`, `Update(src/lib.rs)` of `src/x.rs:12-30`: de code-viewer opent op die regel (alleen lezen). Relatieve paden worden gezocht in de map van de terminal en daarna in de projectroot |
| Van commando naar commando springen (shell-integratie) | **Ctrl+Shift+↑ / ↓**. Elke prompt krijgt een balkje in de linkermarge: mint = gelukt, koraal = mislukt (exit-code ≠ 0), cyaan = loopt nog, grijs = onbekend (cmd meldt geen exit-codes) |
| Uitvoer van het laatste commando kopiëren | Terminal → Copy Last Command Output (prompt, commando en uitvoer; max. 2000 regels, het eind blijft) |
| Die uitvoer bij de prompt van Claude Code / Codex zetten | AI → Insert Last Command Output into Agent: als Markdown-blok met commando en exit-code, **zonder Enter**; het agent-tabblad krijgt de focus zodat je je vraag erbij typt |
| Zien welk tabblad op je wacht | badge op de tab: *needs you* / *done* / *failed* / *working* (programma's die OSC 7501 melden) of *bell* (bel in een tab waar je niet naar kijkt). Is het venster niet actief, dan knippert de taakbalkknop. Verdwijnt zodra je de tab bekijkt |
| Zoeken in terminaluitvoer en scrollback | **Ctrl+Shift+F** (typen zoekt meteen; Enter/F3 ouder, Shift+F3 nieuwer, Ctrl+C kopieert de treffer, Esc sluit) |
| Kopiëren | **Ctrl+C** als er een selectie is (zonder selectie gaat Ctrl+C als onderbreking naar het programma), Ctrl+Shift+C |
| Plakken | **Ctrl+V** (Windows), Ctrl+Shift+V, Shift+Insert, rechtermuisklik. Meerdere regels zonder bracketed paste vragen eerst bevestiging. Op Linux blijft Ctrl+V van het programma (zoals in andere Linux-terminals) |
| Knippen | Niet in de terminal: terminaltekst is uitvoer en kan niet worden weggeknipt. In tekstvelden (palette) werken Ctrl+X/C/V gewoon |
| Scrollback | Muiswiel, Shift+PageUp / Shift+PageDown |
| Lettergrootte | Ctrl+= / Ctrl+- / Ctrl+0 |
| Shell herstarten na een fout-exit | Enter (een schone `exit` sluit de pane) |

## Prompt Studio

Sjablonen met `{{variabelen}}`. ForgeTerm vult in wat het echt weet (`project`, `project_path`, `cwd`, `selection` = geselecteerde terminaltekst, `file` = in de verkenner geselecteerd bestand, relatief aan de projectroot); al het andere typ je zelf. **Insert (no Enter)** zet de tekst bij de prompt van de gefocuste terminal; jij beslist wanneer je verstuurt. Meerregelige tekst in een shell zonder bracketed paste vraagt eerst bevestiging. **Copy** zet hem op het klembord.

**Naast de terminal:** met *Dock beside terminal* wordt de studio een paneel rechts van de terminal. De terminal blijft gewoon werken: klik in een veld om te typen (cyaan rand, cursor in de terminal wordt hol), **Esc** of een klik in de terminal geeft het toetsenbord terug, **Tab** springt tussen velden. *Insert* zet de tekst in de terminal die op dat moment focus heeft en laat het paneel open, zodat je meteen het volgende sjabloon kunt invullen. *Float* maakt er weer een zwevend venster van. De plek wordt onthouden in de sessie (met `restore_session`); ingevulde waarden niet.

Eigen sjablonen: `.md`-bestanden in de map `prompts` naast `settings.toml` (AI → Open Prompts Folder maakt de map met een voorbeeld). Optionele front matter `name:` en `description:`; een eigen bestand met dezelfde naam als een ingebouwd sjabloon (bijv. `review-file.md`) vervangt dat.

## Code-viewer

Bekijk bestanden naast je terminals, alleen lezen: syntaxkleuring (Rust, Python, JS, C/C++, C#, Go, Java, JSON, YAML, Markdown, shell, HTML, CSS, SQL, Makefile, …; onbekend = platte tekst), regelnummers en een **minimap** (code glance) rechts: klik om erheen te springen, sleep om te scrollen. Meerdere bestanden als tabbladen. Klik en sleep om regels te selecteren (Shift+klik breidt uit), **Ctrl+C** kopieert ze, Ctrl+A alles, **Ctrl+F** zoekt (Enter/F3 volgende, Shift+F3 vorige, Esc sluit). Pijltjes, PgUp/PgDn, Home/End scrollen zolang de viewer de focus heeft (klik in de terminal geeft de focus terug). Een bestand dat buiten ForgeTerm wijzigt, wordt automatisch opnieuw geladen. Limieten: tot 8 MB, kleuring tot 2 MB / 50.000 regels, binaire bestanden worden geweigerd; UTF-8 en UTF-16 (met BOM) worden herkend, besturingstekens zichtbaar gemaakt (nooit uitgevoerd).

## Git

De statusbalk toont branch, ahead/behind en het aantal gewijzigde bestanden van de map van de gefocuste terminal (elke 4 s en bij elke mapwissel bijgewerkt). Het Git-paneel toont conflicten, staged, changes en untracked bestanden; klik op een bestand voor de diff (Esc sluit), **+** / **−** staget of unstaget, en *Commit* voert `git commit -m` uit met de hooks van de repository. **Branches**: wisselen (vraagt eerst bevestiging; git weigert als lokale wijzigingen overschreven zouden worden en die melding wordt getoond), nieuwe branch vanaf de huidige commit, remote branch volgen (*Track*). **History & activity** (knop in het paneel, Git-menu of palette): commits per dag over 30 dagen (uit `git log`, lokale kalenderdag), de laatste 300 commits en per commit header, bestandsoverzicht en diff. Push, pull, discard en verwijderen van branches gaan bewust via de terminal.

Veilig in onbekende repositories: lezen voert nooit door de repository geconfigureerde programma's uit (`core.fsmonitor` uit, diffs met `--no-ext-diff --no-textconv`), achtergrond-status neemt geen `index.lock` (`--no-optional-locks`), git vraagt nooit om invoer. Vereist `git` op PATH.

## Installeren

**Windows 11** — `ForgeTerm-<versie>-setup-x64.exe`: installeert per gebruiker (geen beheerdersrechten) naar `%LOCALAPPDATA%\Programs\ForgeTerm`, met snelkoppeling in het Startmenu (bureaublad optioneel) en een vermelding onder *Instellingen › Apps*. Verwijderen haalt alleen de geïnstalleerde bestanden weg; je instellingen en sessie in `%APPDATA%\ForgeTerm` blijven staan. Draait ForgeTerm nog, dan vraagt de installer of verwijderaar eerst om het te sluiten. De installer is niet ondertekend: SmartScreen meldt "Onbekende uitgever" (*Meer info › Toch uitvoeren*). Zonder installer: de zip uitpakken en `forgeterm.exe` starten.

**Linux Mint 22 / Ubuntu 24.04** — `forgeterm_<versie>-1_amd64.deb`:

```
sudo apt install ./forgeterm_0.1.0-1_amd64.deb     # verwijderen: sudo apt remove forgeterm
```

Installeert `/usr/bin/forgeterm`, een menu-item (*Systeemgereedschap / Ontwikkeling*) met icoon, en de documentatie in `/usr/share/doc/forgeterm`. apt haalt ontbrekende bibliotheken zelf op (X11/Wayland, xkbcommon, EGL; aanbevolen: git en Vulkan). Getest in WSL Ubuntu 24.04 (pakketinhoud, afhankelijkheden via `apt-get install --simulate`, starten en Wayland-app-id); nog niet op een echte Mint-desktop.

Controleer een download met het bijbehorende `.sha256`-bestand (`Get-FileHash` / `sha256sum`).

## Release bouwen

Windows (zip, en de installer als [NSIS 3](https://nsis.sourceforge.io) aanwezig is: op PATH, in de standaardmap, of `-Makensis <pad>`):

```
pwsh scripts\package-windows.ps1
```

Maakt `dist\ForgeTerm-<versie>-windows-x64.zip` en `dist\ForgeTerm-<versie>-setup-x64.exe` (elk + `.sha256`) met `forgeterm.exe` (met icoon, zonder consolevenster), deze README, de lettertypelicentie en `THIRD-PARTY.txt`: alle meegecompileerde crates met hun licentie, gegenereerd uit `cargo tree`. Het installerscript staat in `packaging/windows/forgeterm.nsi`.

Linux (.deb; vereist `cargo install --locked cargo-deb` en `dpkg-dev`):

```
bash scripts/package-linux.sh
```

Maakt `dist/forgeterm_<versie>-1_amd64.deb` (+ `.sha256`). Pakketgegevens: `[package.metadata.deb]` in `crates/forgeterm-app/Cargo.toml`; menu-item: `packaging/linux/forgeterm.desktop`.

## Ontwikkelen in VS Code

Open de map in VS Code; aanbevolen extensies worden voorgesteld (`.vscode/extensions.json`).

| Actie | Hoe |
|---|---|
| Bouwen | `Ctrl+Shift+B` |
| Debuggen | `F5` → *ForgeTerm — Debug (testinstellingen)*: gebruikt `target/dev/settings.toml`, je echte instellingen blijven onaangeroerd. *Debug (eigen instellingen)* gebruikt je gewone `settings.toml` |
| Release testen | `F5` → *ForgeTerm — Release (testinstellingen)*; log in `target/forgeterm-release.log` |
| Starten zonder debugger | Task *ForgeTerm starten* / *ForgeTerm starten (testinstellingen)* |
| Tests | *Tasks: Run Test Task*, de Testing-zijbalk, of de ▶ Run / Debug-lens boven een test |
| Tests debuggen | `F5` → *Tests debuggen: library-crate kiezen* of *Tests debuggen: app* (vraagt crate + testfilter) |
| Kwaliteitspoort | Task *Kwaliteitspoort*: fmt-check → clippy → clippy Linux-target → alle tests |
| Overig | Tasks *Probe*, *cargo doc (openen)*, *Windows-pakket bouwen* |

Tasks start je via `Ctrl+Shift+P` → *Tasks: Run Task*. Opslaan formatteert automatisch (rustfmt) en zet regeleindes op LF. rust-analyzer draait clippy bij opslaan en gebruikt een eigen target-map, zodat het de build niet blokkeert. Voor Linux-code (`#[cfg(unix)]`) met volledige IDE-hulp: zet tijdelijk `rust-analyzer.cargo.target` in `.vscode/settings.json`.

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
| `terminal.shell_integration` | true | shells melden hun map en markeren prompts en commando's (geen profielwijzigingen) |
| `explorer.visible` / `follow_terminal` / `show_hidden` | false / true / false | verkenner |
| `[keys]` | (leeg) | eigen sneltoetsen: `"Ctrl+K" = "command_palette"`, `= "none"` haalt een standaardtoets weg; namen staan in Help → Keyboard Shortcuts |
| `workspace.restore_session` | true | bij het starten de tabs, splits (met hun map) en viewer-bestanden van de vorige keer terugzetten |

Fouten (typefout, onbekende sleutel, waarde buiten bereik) verschijnen in een balk bovenin; die instelling valt terug op de standaard. Een syntaxfout tijdens het bewerken laat de huidige instellingen staan. ForgeTerm schrijft nooit in een bestaand instellingenbestand.

**Sessie:** de indeling staat in `session.toml` naast `settings.toml` (alleen shell, map en splitverhouding per pane, de actieve tab en de open viewer-bestanden; nooit terminaluitvoer of commando's). Agents komen terug als gewone shell in hun projectmap, zodat er nooit vanzelf een agent start. Verwijderen kan altijd; dan start ForgeTerm met één tab.
