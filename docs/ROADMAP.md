# ForgeTerm — Roadmap

Legenda: ✅ klaar en getest · 🟡 deels · ⬜ open. "Getest" betekent: daadwerkelijk uitgevoerd op het genoemde platform.

## Fase 1 — Fundament

| Item | Status | Bewijs |
|---|---|---|
| Rust-toolchain + MSVC Build Tools + Windows SDK | ✅ | rustc 1.99.1, VS 2022 Build Tools, SDK 10.0.26100 |
| Library-onderzoek + architectuur | ✅ | docs/ARCHITECTURE.md |
| Cross-platform PTY-prototype | 🟡 | Windows ConPTY ✅ (integratietest + handmatig). Linux: compileert + clippy schoon voor x86_64-unknown-linux-gnu, ⬜ niet uitgevoerd. |
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
| Workspace-opslag (tabs/splits bewaren, SQLite) | ⬜ |

## Fase 3 — Premium UI

Design system ✅ (tokens), thema's ⬜ (alleen Forge Dark), Theme Editor ⬜, animaties ⬜, Command Palette ✅, splashscreen ⬜ (met reduced-motion en uitschakelbaar), logo ✅ (eerste versie, `assets/branding`).

## Fase 4 — Analytics

`forgeterm-analytics` (sysinfo, ringbuffers) ⬜ · `forgeterm-charts` (eigen canvas-chartlaag) ⬜ · `forge-monitor` CLI ⬜ · GPU via NVML/ADLX/sysfs met eerlijke "niet beschikbaar"-melding ⬜.

## Fase 5 — AI Workspace

Agent Adapter System: detectie + versie ✅ Windows (GUI: Claude Code 2.1.322, codex-cli 0.167.0 gevonden); starten in projectroot als tab/split ✅ Windows · Claude Code-adapter: projectcontext (CLAUDE.md, rules, skills, commands, subagents, settings, MCP) ✅ Windows (GUI), `@pad` invoegen ✅ Windows · Codex-adapter (detectie ✅, start ✅ Windows, AGENTS.md ✅) · Prompt Studio met ingebouwde + eigen sjablonen, `{{variabelen}}`, automatische waarden uit echte state en invoegen zonder Enter ✅ Windows (GUI P1–P6) · Hooks/MCP-details tonen ⬜ · Sessie-/tokeninformatie ⬜ (alleen met gedocumenteerde bron).

## Fase 6 — Git en Advanced

Code-viewer met syntaxkleuring, minimap (code glance), tabbladen, zoeken, kopiëren en automatisch herladen ✅ Windows · bewerken/opslaan ⬜ (bewust later).

Git Control Center: statusbalk (branch, ahead/behind, wijzigingen), paneel met diff, stage/unstage, commit, recente commits ✅ Windows · branches (wisselen met bevestiging, nieuw, volgen) ✅ Windows · history + commitdetails ✅ Windows · Git Activity Chart (commits per dag, 30 dagen) ✅ Windows · push/pull/discard ⬜ (bewust terminal) · layouts ⬜ · sneltoetsbeheer ⬜.

## Fase 7 — Release

Linux Mint-validatie ⬜ · installers (MSI/winget, .deb/AppImage) ⬜ · benchmarks ⬜ · security review ⬜ · documentatie ⬜.

## Bekende beperkingen (eerlijk)

- **Linux niet getest.** Code-paden bestaan, maar er is geen validatie op Linux Mint gedaan.
- **Kitty keyboard protocol uit.** Legacy encoding: Shift+Enter is niet te onderscheiden van Enter. In Claude Code werkt `\` + Enter voor een nieuwe regel.
- **Emoji monochroom** (fallback-font zonder kleur-glyphs).
- **Geen IME-ondersteuning** (Chinees/Japans invoeren). Dode toetsen (US-International) werken wél.
- **Celbreedte afgeleid van JetBrains Mono** (0,6 em). Een andere font vereist meting van de advance — gepland bij font-instellingen.
- **Muismodifiers** (Shift/Ctrl bij muisrapportage) worden nog niet meegestuurd.
- **Geen cursor-knipperen**, geen URL-detectie, geen zoekfunctie in output.
- **Werkmap volgen** werkt in PowerShell 7, Windows PowerShell, cmd en Git Bash (getest). Niet voor zsh (vereist een hook in de eigen config), WSL (Windows-omgeving gaat niet mee) en fish (alleen als fish zelf OSC 7 stuurt). Een eigen rc die `PROMPT_COMMAND` overschrijft, schakelt het voor bash uit. Wat een programma tijdens het draaien als map meldt, wordt overgenomen als het een bestaande map is.
- **Tabbalk** scrolt horizontaal bij veel tabs; de positie van de actieve tab is een benadering (tabs verschillen in breedte).
- **Explorer:** geen toetsenbordnavigatie in de boom, geen automatische verversing bij wijzigingen (knop Refresh), geen bestandsbewerkingen (bewust: alleen lezen), root start in de map van ForgeTerm (geen mapkiezer). Paneelbreedte vast (270 px).
- **cmd en `%VAR%`:** in interactieve cmd wordt `%NAAM%` ook binnen quotes uitgebreid als die variabele bestaat; dat is in cmd niet te escapen.
- **WSL-quoting** (`/mnt/c/…`) is unit-getest maar niet in een echte WSL-shell uitgevoerd.
- **Instellingen** worden niet vanuit de UI geschreven (zoom/verkenner-toggles gelden voor de sessie). Geen instellingen-UI; het bestand is de bron. Nog geen thema-, lettertype-familie- of sneltoetsinstellingen.
- **Afsluiten vraagt geen bevestiging** (menu Quit en venster-×), ook niet als er nog processen lopen.
- **Geen Alt-mnemonics in de menubalk** (Alt+F enz. horen bij de shell); menu's openen met de muis of via de Command Palette.
- **Tab-sneltoets Alt+1…9** i.p.v. Ctrl+Alt+N: Ctrl+Alt is AltGr op Windows en typt tekens op veel layouts.
