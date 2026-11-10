# ForgeTerm — Roadmap

Legenda: ✅ klaar en getest · 🟡 deels · ⬜ open. "Getest" betekent: daadwerkelijk uitgevoerd op het genoemde platform.

## Fase 1 — Fundament

| Item | Status | Bewijs |
|---|---|---|
| Rust-toolchain + MSVC Build Tools + Windows SDK | ✅ | rustc 1.99.1, VS 2022 Build Tools, SDK 10.0.26100 |
| Library-onderzoek + architectuur | ✅ | docs/ARCHITECTURE.md |
| Cross-platform PTY-prototype | ✅ | Windows ConPTY ✅. Linux (WSL Ubuntu 24.04, basis van Mint 22): build + clippy + 142 tests (echte PTY) ✅, GUI onder WSLg ✅. Echte Mint-desktop ⬜ |
| Terminalemulatie valideren | ✅ | Kleuren 16/256/truecolor, attributen, CJK, combining, alt screen (vim), resize |
| **Verticale slice: venster → PTY → rendering → shell → Claude Code** | ✅ Windows | Claude Code start, rendert, reageert op pijltjes/Esc |

## Fase 2 — Core Workspace

| Item | Status |
|---|---|
| Hoofdvenster met app-balk en statusbalk | ✅ |
| Terminaltabs | ✅ Windows (2026-11-10) |
| Splits (naast/onder elkaar, resize, focus) | ✅ Windows (2026-11-10) |
| Shellkeuze per tab (pwsh, Windows PowerShell, cmd, Git Bash, WSL) | ✅ — WSL wordt gedetecteerd maar is nog niet in ForgeTerm getest |
| Panes verslepen naar andere positie | ⬜ (vereist pane-titelbalk) |
| Maximaliseren van één pane (zoom) | ✅ Ctrl+Shift+Enter |
| Centrale command registry (acties + sneltoetsen op één plek) | ✅ |
| Command Palette (Ctrl+Shift+P, fuzzy, recent, alleen toepasbare acties) | ✅ Windows (2026-11-10) |
| Menubalk (leest uit de registry; File/Edit/View/Terminal/Help, About-venster) | ✅ Windows (2026-11-10) |
| File Explorer (zijpaneel, lazy boom, verborgen bestanden, refresh, map omhoog / als root, nieuwe tab in map) | ✅ Windows (2026-11-10) |
| Werkmap volgen: nieuwe tab/split/herstart in de map van de shell, verkenner volgt, map in statusbalk | ✅ Windows (2026-11-10): pwsh, Windows PowerShell, cmd, Git Bash |
| Drag-and-drop bestanden → terminal (shell-specifieke escaping) | ✅ Windows: vanuit Windows Verkenner; quoting getest in pwsh, Windows PowerShell, cmd, Git Bash. Slepen vanuit het eigen explorer-paneel ⬜ (nu dubbelklik / *Insert path*) |
| Instellingen (`settings.toml`, live herladen, foutmeldingen) | ✅ Windows (2026-11-10) — zie TEST_PLAN T1–T8 |
| Workspace-opslag (tabs, splits met verhouding, map per pane, actieve tab, viewer-bestanden in `session.toml`) | ✅ Windows (2026-11-10) — geen SQLite nodig |

## Fase 3 — Premium UI

Design system ✅ (tokens), thema's ⬜ (alleen Forge Dark), Theme Editor ⬜, animaties ⬜, Command Palette ✅, splashscreen ⬜ (met reduced-motion en uitschakelbaar), logo ✅ (eerste versie, `assets/branding`).

## Fase 4 — Analytics

Monitor-paneel (Ctrl+Shift+M): CPU en geheugen van het systeem met 3 minuten historie (sparklines met hover), per terminal CPU/geheugen/processen van de hele procesboom van de shell — gemeten via sysinfo (`forgeterm-system::Sampler`), alleen zolang het paneel open is ✅ · GPU: toont eerlijk "not measured" ⬜ (NVML/ADLX/sysfs) · `forge-monitor` CLI ⬜.

## Fase 5 — AI Workspace

Agent Adapter System: detectie + versie ✅ Windows (GUI: Claude Code 2.1.322, codex-cli 0.167.0 gevonden); starten in projectroot als tab/split ✅ Windows · Claude Code-adapter: projectcontext (CLAUDE.md, rules, skills, commands, subagents, settings, MCP) ✅ Windows (GUI), `@pad` invoegen ✅ Windows · Codex-adapter (detectie ✅, start ✅ Windows, AGENTS.md ✅) · Prompt Studio met ingebouwde + eigen sjablonen, `{{variabelen}}`, automatische waarden uit echte state en invoegen zonder Enter ✅ Windows (GUI P1–P6) · Hooks/MCP-details tonen ⬜ · Sessie-/tokeninformatie ⬜ (alleen met gedocumenteerde bron).

## Fase 6 — Git en Advanced

Code-viewer met syntaxkleuring, minimap (code glance), tabbladen, zoeken, kopiëren en automatisch herladen ✅ Windows · bewerken/opslaan ⬜ (bewust later).

Git Control Center: statusbalk (branch, ahead/behind, wijzigingen), paneel met diff, stage/unstage, commit, recente commits ✅ Windows · branches (wisselen met bevestiging, nieuw, volgen) ✅ Windows · history + commitdetails ✅ Windows · Git Activity Chart (commits per dag, 30 dagen) ✅ Windows · push/pull/discard ⬜ (bewust terminal) · layouts ✅ (sessie herstellen) · sneltoetsbeheer ✅ (`[keys]` + Help → Keyboard Shortcuts).

## Fase 7 — Release

App-icoon: venster (gerenderd uit het SVG-logo) en `forgeterm.exe` (ICO via build.rs + Windows SDK rc) ✅ Windows · Linux-validatie in WSL Ubuntu 24.04 ✅ (echte Mint-desktop ⬜) · draagbare Windows-zip met licentielijst en SHA-256 (`scripts/package-windows.ps1`) ✅ · Linux-pakket `.deb` voor Mint 22 / Ubuntu 24.04 (`scripts/package-linux.sh`, menu-item + icoon, Wayland-app-id) ✅ WSL · Windows-installer per gebruiker (NSIS, `packaging/windows/forgeterm.nsi`; installeren, opnieuw installeren, verwijderen met behoud van instellingen) ✅ · code-signing ⬜ · AppImage/winget ⬜ · geheugenmeting release ✅ (163 MB idle, binnen budget) · geautomatiseerde benchmarks ⬜ · security review ✅ (2026-11-10, 5 bevindingen opgelost, zie ARCHITECTURE §6) · documentatie 🟡 (README als handleiding, ARCHITECTURE, TEST_PLAN; aparte gebruikershandleiding ⬜).

## Bekende beperkingen (eerlijk)

- **Linux getest in WSL Ubuntu 24.04** (dezelfde basis als Linux Mint 22): build, alle tests en de GUI onder WSLg. Een echte Mint-desktop (Cinnamon/X11) is nog niet getest.
- **Kitty keyboard protocol** staat aan (vlaggen 1, 8 en 16 geïmplementeerd; 2 = alleen indrukken, 4 = alternatieve toetsen weggelaten, beide toegestaan). Claude Code vraagt vlaggen 5 aan, dus **Shift+Enter = nieuwe regel** in Claude Code. Programma's die het protocol niet aanzetten, krijgen de gewone (legacy) codering. Crasht een programma zonder het protocol uit te zetten, dan krijgt de shell daarna voor Ctrl/Alt-combinaties, Esc en Shift+Enter CSI-u-reeksen tot de pane opnieuw start.
- **Emoji monochroom** (fallback-font zonder kleur-glyphs).
- **Geen IME-ondersteuning** (Chinees/Japans invoeren). Dode toetsen (US-International) werken wél.
- **Celbreedte afgeleid van JetBrains Mono** (0,6 em). Een andere font vereist meting van de advance — gepland bij font-instellingen.
- **Muismodifiers** (Shift/Alt/Ctrl bij muisrapportage) worden meegestuurd (unit-getest). Op Windows zet ConPTY VT-muisinvoer om, waardoor programma's onder ConPTY de rapportage niet als bytes zien; een GUI-validatie op Linux staat nog open.
- **Geen cursor-knipperen.** Zoeken in de output vindt één treffer per keer (geen totaal aantal). Links worden per schermregel herkend: een link die over twee regels doorloopt, wordt niet als geheel gezien. Alleen http(s)-links (bewust).
- **Werkmap volgen** werkt in PowerShell 7, Windows PowerShell, cmd en Git Bash (getest). Niet voor zsh (vereist een hook in de eigen config), WSL (Windows-omgeving gaat niet mee) en fish (alleen als fish zelf OSC 7 stuurt). Een eigen rc die `PROMPT_COMMAND` overschrijft, schakelt het voor bash uit. Wat een programma tijdens het draaien als map meldt, wordt overgenomen als het een bestaande map is.
- **Tabbalk** scrolt horizontaal bij veel tabs; de positie van de actieve tab is een benadering (tabs verschillen in breedte).
- **Explorer:** geen toetsenbordnavigatie in de boom, geen automatische verversing bij wijzigingen (knop Refresh), geen bestandsbewerkingen (bewust: alleen lezen), root start in de map van ForgeTerm (geen mapkiezer). Paneelbreedte vast (270 px).
- **cmd en `%VAR%`:** in interactieve cmd wordt `%NAAM%` ook binnen quotes uitgebreid als die variabele bestaat; dat is in cmd niet te escapen.
- **WSL-quoting** (`/mnt/c/…`) is unit-getest maar niet in een echte WSL-shell uitgevoerd.
- **Instellingen** worden niet vanuit de UI geschreven (zoom/verkenner-toggles gelden voor de sessie). Geen instellingen-UI; het bestand is de bron. Nog geen thema-, lettertype-familie- of sneltoetsinstellingen.
- **Afsluiten** vraagt alleen bevestiging als er in een shell nog een programma draait (kindproces van de shell); een shell die alleen op de prompt wacht, sluit direct. Programma's die niet als kind van de shell lopen (bijv. losgekoppeld gestart), worden niet gezien.
- **Geen Alt-mnemonics in de menubalk** (Alt+F enz. horen bij de shell); menu's openen met de muis of via de Command Palette.
- **Tab-sneltoets Alt+1…9** i.p.v. Ctrl+Alt+N: Ctrl+Alt is AltGr op Windows en typt tekens op veel layouts.
