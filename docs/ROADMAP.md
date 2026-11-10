# ForgeTerm — Roadmap

Legenda: ✅ klaar en getest · 🟡 deels · ⬜ open. "Getest" betekent: daadwerkelijk uitgevoerd op het genoemde platform.

## Fase 1 — Fundament

| Item | Status | Bewijs |
|---|---|---|
| Rust-toolchain + MSVC Build Tools + Windows SDK | ✅ | rustc 1.99.1, VS 2022 Build Tools, SDK 10.0.26100 |
| Library-onderzoek + architectuur | ✅ | docs/ARCHITECTURE.md |
| Cross-platform PTY-prototype | 🟡 | Windows ConPTY ✅ (integratietest + handmatig). Linux ⬜ niet getest. |
| Terminalemulatie valideren | ✅ | Kleuren 16/256/truecolor, attributen, CJK, combining, alt screen (vim), resize |
| **Verticale slice: venster → PTY → rendering → shell → Claude Code** | ✅ Windows | Claude Code start, rendert, reageert op pijltjes/Esc |

## Fase 2 — Core Workspace

| Item | Status |
|---|---|
| Hoofdvenster met app-balk en statusbalk | ✅ |
| Terminaltabs | ⬜ volgende stap |
| Splits | ⬜ |
| Shellkeuze (detectie bestaat al in engine) | 🟡 engine klaar, UI ⬜ |
| File Explorer | ⬜ |
| Drag-and-drop bestanden → terminal (shell-specifieke escaping) | ⬜ |
| Menubalk + centrale command registry | ⬜ |
| Instellingen (TOML) + workspace-opslag (SQLite) | ⬜ |

## Fase 3 — Premium UI

Design system ✅ (tokens), thema's ⬜ (alleen Forge Dark), Theme Editor ⬜, animaties ⬜, Command Palette ⬜, splashscreen ⬜ (met reduced-motion en uitschakelbaar), logo ✅ (eerste versie, `assets/branding`).

## Fase 4 — Analytics

`forgeterm-analytics` (sysinfo, ringbuffers) ⬜ · `forgeterm-charts` (eigen canvas-chartlaag) ⬜ · `forge-monitor` CLI ⬜ · GPU via NVML/ADLX/sysfs met eerlijke "niet beschikbaar"-melding ⬜.

## Fase 5 — AI Workspace

Agent Adapter System (detectie, versie, start in projectmap) ⬜ · Prompt Studio met presets en variabelen ⬜ · Claude Code-adapter (CLAUDE.md, slash commands, skills, hooks — alleen gedocumenteerde interfaces) ⬜ · Codex-adapter ⬜.

## Fase 6 — Git en Advanced

Git Control Center ⬜ · Git Activity Charts ⬜ · layouts ⬜ · sneltoetsbeheer ⬜.

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
