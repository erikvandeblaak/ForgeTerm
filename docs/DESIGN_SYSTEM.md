# ForgeTerm — Design System

Bron in code: `crates/forgeterm-app/src/theme.rs` (UI) en `crates/forgeterm-terminal/src/palette.rs` (terminal). Wijzig kleuren alleen daar.

## Principes

Beautiful by default · keyboard-first · minimale visuele ruis · maximale informatiedichtheid · geen nepfunctionaliteit (een knop die niets doet bestaat niet).

## Oppervlakken

| Token | Hex | Gebruik |
|---|---|---|
| `OBSIDIAN` | `#090D14` | Terminalachtergrond, app-achtergrond |
| `GRAPHITE` | `#111827` | App-balk, statusbalk |
| `SURFACE` | `#172132` | Banners, panelen |
| `ELEVATED` | `#202C40` | Knoppen, popovers, ANSI black |
| `BORDER` | `#253247` | 1px scheidingslijnen |

## Accenten en semantiek

| Token | Hex | Semantiek |
|---|---|---|
| `CYAN` Electric Cyan | `#4DD8E8` | Primair, focus, cursor, selectie-accent |
| `MINT` Emerald Mint | `#55E6B5` | Succes, "Running" |
| `VIOLET` Royal Violet | `#A78BFA` | Informatief, scrollback-indicator |
| `AMBER` Warm Amber | `#F5C978` | Waarschuwing (paste-bevestiging, "Exited") |
| `CORAL` Coral Red | `#F08080` | Fout |
| `TEXT` | `#D6DEEB` | Primaire tekst (contrast 14.4:1 op Obsidian) |
| `TEXT_MUTED` | `#8B98AD` | Secundaire tekst (contrast 6.7:1 op Obsidian, 6.1:1 op Graphite — beide ≥ WCAG AA) |

Disabled: `DISABLED` `#5F6B80` (3.0:1 op Surface; WCAG stelt geen eis aan uitgeschakelde bedieningselementen, maar het blijft leesbaar). Geselecteerd (terminal): bg `#2B4466`, fg `#F2F6FB`.

## Terminalpalet "Forge Dark"

| # | Normaal | Helder |
|---|---|---|
| 0 black | `#202C40` | `#5B6B85` |
| 1 red | `#F08080` | `#FF9E9E` |
| 2 green | `#55E6B5` | `#7DF0C8` |
| 3 yellow | `#F5C978` | `#FFDDA0` |
| 4 blue | `#3B6FD8` | `#93C1FF` |
| 5 magenta | `#A78BFA` | `#C4B2FF` |
| 6 cyan | `#4DD8E8` | `#82E8F2` |
| 7 white | `#C8D3E3` | `#F2F6FB` |

Blauw is bewust donkerder dan de rest: programma's (o.a. PowerShell `-BackgroundColor DarkBlue`) gebruiken het als achtergrond. Gemeten: tekstkleur op `#3B6FD8` = 3.5:1; rood op blauw blijft 1.8:1 — dat is in vrijwel ieder palet zwak. Open punt: optioneel "minimum contrast"-mechanisme (zoals in moderne terminals) dat fg automatisch bijstuurt.

## Typografie

- Terminal: **JetBrains Mono** (Regular/Bold/Italic, OFL-1.1, gebundeld), standaard 14 px, Ctrl+= / Ctrl+- / Ctrl+0.
- UI: **Fira Sans** (iced-ingebouwd) — tijdelijk; Inter (OFL) volgt bij de premium UI-fase.
- Groottes UI: 15 (titel), 13 (primair), 12 (secundair).

## Ruimte en vorm

Padding app-balk 8×14, statusbalk 5×14, terminal 10 rondom. Knopradius 6. Randen 1 px.

## Logo

`assets/branding/forgeterm-logo.svg`: geometrische "F" met cyaan→violet verloop in een afgeronde graphite tegel; de onderste arm is een mint terminalcursor. Gebruik: app-balk (22 px), later splash/about/installer.

## Signature

"Erik van de Blaak Edition" in de app-balk (muted, 12 px) en "Created by Erik van de Blaak" rechts in de statusbalk. Ingetogen; nooit in accentkleur.
