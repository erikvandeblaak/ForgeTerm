# ForgeTerm — Design System

Bron in code: `crates/forgeterm-app/src/theme.rs`. Daar staan per thema de UI-kleuren, het terminalpalet en de codekleuren, plus de maten voor tekst, ruimte en radius. Wijzig kleuren alleen daar. `Palette::forge_dark()` in `forgeterm-terminal` is de standaard van de engine; een test bewaakt dat Midnight gelijk is.

## Thema's

| Thema | Karakter | Instelling |
|---|---|---|
| **Midnight** | diep blauwzwart met cyaan; de oorspronkelijke Forge Dark (standaard) | `midnight` |
| **Graphite** | neutraal donkergrijs zonder blauwzweem, rustigere accenten | `graphite` |
| **Arctic** | licht; donkerdere accenten zodat ze op wit leesbaar zijn | `arctic` |
| **Cyber** | violetzwart met neonaccenten | `cyber` |
| **High Contrast** | zwart en wit, felle accenten, randen die opvallen | `high-contrast` |

- **Kiezen:** `[appearance] theme = "…"` in `settings.toml` is de standaard. *View → Theme: …* (of de palette) probeert er een voor deze sessie, net als de lettergrootte. ForgeTerm schrijft nooit in het instellingenbestand.
- **Wisselen** gebeurt zonder herstart: de UI, alle open terminals (en hun miniaturen in Mission Control) en de code-viewer krijgen direct de nieuwe kleuren. Programma's die naar de kleuren vragen (OSC 10/11) krijgen het nieuwe antwoord. Op Windows laat ConPTY die antwoorden vallen; op Linux werkt het (getest).
- **Code-kleuring** levert rollen (commentaar, string, keyword, …); de app kiest de kleur per thema, dus wisselen kost geen nieuwe kleuring.
- **Dezelfde semantiek** in elk thema: cyaan = primair/focus, mint = succes, violet = informatief, amber = waarschuwing, coral = fout. Gevulde accentknoppen dragen de achtergrondkleur als tekst (licht op donker accent in Arctic).
- **Contrast** wordt per thema getest (`theme::tests::contrast_meets_the_design_system`):
  - tekst ≥ 7:1 en gedempte tekst ≥ 4,5:1 op alle vier de oppervlakken;
  - accenten ≥ 3:1 op die oppervlakken;
  - tekst op selectie ≥ 4,5:1, en tekst op een cyaan knop ≥ 4,5:1;
  - grafiekbalken ≥ 3:1;
  - in de terminal: voorgrond ≥ 7:1 en élke ANSI-kleur ≥ 3:1 (zwart uitgezonderd in donkere thema's, waar programma's het als achtergrond gebruiken);
  - codekleuren ≥ 3:1.
- **Arctic:** ANSI wit en helder wit zijn middengrijs, zoals in de lichte thema's van VS Code. Programma's (PSReadLine, git) printen gewone tekst in "wit"; op een lichte achtergrond was dat onleesbaar (gezien in de GUI-test).
- **Reduced motion:** ForgeTerm heeft geen animaties (geen knipperende cursor, geen overgangen, geen spinners). Een instelling daarvoor zou niets doen en komt er pas met de eerste animatie.

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
- Groottes UI (`theme::size`): `CAPTION` 10 (badges, as-labels), `TINY` 11 (tellers, hints), `SMALL` 12 (labels, lijstregels, knoppen), `BODY` 13 (primair), `LARGE` 14, `TITLE` 15 (dialoogtitels), `HEADING` 18 (koppen in dashboards). Eenmalige grote maten (grote getallen, het logo in About) staan bij hun gebruik.

## Ruimte en vorm

- Afstand tussen items (`theme::space`): `XXS` 2, `XS` 4, `S` 6, `M` 8, `L` 10, `XL` 12, `XXL` 16.
- Radius (`theme::radius`): `S` 4 (badges, chips), `M` 6 (knoppen, invoer, menuregels), `L` 8 (popovers, kaarten), `XL` 10 (panelen, dialogen). Ronde stippen gebruiken de helft van hun maat.
- Padding staat nog bij de widgets (app-balk 8×14, statusbalk 5×14, terminal 10 rondom). Randen 1 px.

## Logo

`assets/branding/forgeterm-logo.svg`: geometrische "F" met cyaan→violet verloop in een afgeronde graphite tegel; de onderste arm is een mint terminalcursor. Gebruik: app-balk (22 px), later splash/about/installer.

## Signature

"Erik van de Blaak Edition" in de app-balk (muted, 12 px) en "Created by Erik van de Blaak" rechts in de statusbalk. Ingetogen; nooit in accentkleur.
