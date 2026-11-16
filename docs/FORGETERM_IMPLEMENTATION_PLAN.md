# ForgeTerm — Implementatieplan

Datum: 2026-11-16. Bron: `docs/FORGETERM_ARCHITECTURE_AUDIT.md` (bevindingen T*, S*, P*).

**Prioriteiten:**

| Prio | Betekenis |
|---|---|
| **P0** | Veiligheid of stille dataverlies-/functieverlies-bugs; eerst |
| **P1** | Kernwaarde van de masterprompt (AI Mission Control, Git) |
| **P2** | Kwaliteit, performance, onderhoudbaarheid |
| **P3** | Uitbreiding van het platform (fase 4) |

**Werkwijze per milestone:**
- Een feature-branch.
- Acceptatiecriteria vooraf.
- Unit tests voor GUI-vrije logica.
- GUI-validatie met screenshots in een wegwerp-omgeving (`FORGETERM_SETTINGS`/`FORGETERM_SCREENSHOT`).
- Kwaliteitspoort: fmt, clippy (Windows + `x86_64-unknown-linux-gnu`) en alle tests.
- Merge naar `master` alleen na akkoord van Erik.

## P0 — Veiligheid en stille fouten

| Item | Acceptatiecriteria |
|---|---|
| S1 Submodule-filters ✅ | `status`/`diff` met `--ignore-submodules=all` en `-c diff.ignoreSubmodules=all`. Een test met een echte submodule en een `filter.x.clean`-marker bewijst dat er niets draait. |
| S3 Viewer-apparaatbestanden ✅ | `load` weigert alles wat geen gewoon bestand is en leest met `take(MAX+1)`. Test (Unix): symlink naar `/dev/zero` geeft een nette fout. |
| S2 Symlinks naar netwerkpaden ✅ | Verkenner, bestandslinks, OSC 7-`is_dir`, sessieherstel en contextbestanden controleren het doel van een link met `is_local_path` vóór ze hem volgen. Test met een symlink naar `\\127.0.0.1\x`: geen netwerktoegang. |
| T1 Achtergrondpaniek ✅ (algemener opgelost: alle busy-vlaggen terug, zie audit) | `Message::WorkerFailed(kind)` logt en zet de bijbehorende busy-vlag terug; de instellingenbanner blijft. Test: een paniek in een git-taak laat de volgende refresh gewoon starten. |
| T2 Zijpanelen | `enum SidePanel { None, Explorer, Ai, Git, Monitor }` vervangt de vier booleans. Tests voor toggles en voor het laden van instellingen. |
| S4/S5 | Plakbeslissing onder één lock; git-uitvoer begrensd. |

## P1 — Milestone 1: AI Mission Control ✅ (branch `feature/mission-control`, wacht op merge-akkoord)

**Doel:** in één blik zien welke agent wat doet, waar, hoelang, en wanneer je moet ingrijpen.

**Geleverd:**
- **Openen:** `Ctrl+Shift+D` (menu AI, palette).
- **Kaart per agent-terminal,** urgentste eerst, met:
  - statusrail en statuslabel;
  - project, branch (uit `HEAD`, zonder git-proces) en een worktree-label;
  - map, runtime en tijd sinds laatste uitvoer;
  - live miniatuur van de terminal;
  - gemeten CPU (sparkline) en geheugen van de procesboom;
  - de taak uit de venstertitel;
  - acties Open / Files / Changes / Stop.
- **"Recently ended":** exitcode en looptijd van afgelopen sessies.
- **Toetsenbord:** ←/→/Tab selecteren, Enter opent, Esc sluit.
- **Status alleen uit feiten:** OSC 7501 als de agent die meldt, de bel, processtatus en exitcode. Zonder melding staat er "Running", nooit een geraden "idle".
- **Gedragswijziging:** een agent-sessie beëindigen vraagt altijd eerst bevestiging (ook bij tab sluiten en afsluiten).

**Getest:**
- Unit tests:
  - statusmodel, duur/recency/liveness (`forgeterm-agents::mission`);
  - HEAD-lezer met echte repo + worktree (`forgeterm-git::head`);
  - preview-rijen, padinkorting, begrensde historie;
  - command-routing (`shows_terminals`).
- GUI Windows 11, met Claude Code 2.1.325 en codex-cli 0.167.0 in een wegwerp-repo, zonder prompts te versturen:
  - leeg dashboard;
  - twee kaarten;
  - pijl + Enter → tab;
  - Stop → bevestiging → "closed by you";
  - Changes → Git-paneel;
  - Files → verkenner.

**Niet getest / beperkingen:**
- "Needs you", "Working" en "Done" in de GUI: ConPTY gooit het OSC 7501-queryantwoord weg, en Claude Code meldt in onze test niets. Wel unit-getest.
- Linux GUI niet uitgevoerd; alleen clippy voor het Linux-target.

## P1 — Milestone 2: Git Control Center als dashboard ✅ (branch `feature/git-dashboard`, 2026-11-16)

Geleverd: alles hieronder behalve de syntaxgekleurde diff op een canvas (de bestaande diff-overlay wordt gebruikt). S1 (submodules) is meteen meegenomen. Zie ARCHITECTURE §4i2 en TEST_PLAN GD1–GD10.

**Doel:** een visueel overzicht van de repository, en van wat agents erin veranderen.

- **Hoofdweergave** (zoals Mission Control, `Ctrl+Shift+G` blijft het zijpaneel):
  - branch, ahead/behind en upstream;
  - wijzigingen gegroepeerd (staged, unstaged, untracked, conflicten) met diffstat-balkjes;
  - activiteitsgrafiek (bestaand);
  - recente commits als tijdlijn;
  - **worktree-overzicht** (`git worktree list --porcelain`).
- **Per worktree:** branch, aantal wijzigingen en welke agent-terminal erin werkt (koppeling via de map van de terminal: betrouwbaar, want ForgeTerm startte hem daar).
- **Diffviewer met syntaxkleuring:** hergebruik van de viewer-renderer op een canvas, alleen zichtbare regels (lost P4 op voor diffs).
- **Acceptatie:**
  - geen enkele schrijvende git-actie zonder klik;
  - trust-pauze geldt ook hier;
  - unit tests voor de worktree-parser;
  - GUI-screenshots met een wegwerp-repo met 2 worktrees.

## P1 — Milestone 3: Worktree per agent (Parallel AI, deel 1) ✅ (branch `feature/agent-worktrees`, 2026-11-16)

Besluit (standaard, aan te passen): worktrees komen in `<repo>-worktrees/<branch>` naast de repository; nog geen instelling. Zie ARCHITECTURE §4i3.

- "Start agent in nieuwe worktree…":
  - naam valideren (`check-ref-format`);
  - `git worktree add -b <branch> <pad>` na bevestiging (pad standaard naast de repo, nooit in de repo zelf);
  - de agent starten in die map.
- **Worktree verwijderen** alleen als hij schoon is (`git worktree remove`, zonder `--force`). Met wijzigingen: weigeren met de lijst.
- **Besluit nodig (Erik):** waar komen worktrees standaard (`<repo>-worktrees/<branch>` naast de repo?) en wordt dat een instelling?

## P1 — Milestone 4: Vergelijken (Parallel AI, deel 2)

- Twee worktrees naast elkaar:
  - gewijzigde bestanden en diffstat;
  - side-by-side diff per bestand;
  - op verzoek `cargo test` / `cargo clippy` (of de projectspecifieke commando's) in een **zichtbare** terminal per worktree, nooit verborgen.
- **Geen automatisch oordeel:** alleen de cijfers en hun beperkingen.
- **Dezelfde opdracht naar beide agents:** invoegen zonder Enter (projectregel); de gebruiker bevestigt in elke terminal zelf.

## P2 — Kwaliteit en performance

1. **Meten eerst:**
   - `benches/` met criterion voor `Session::snapshot`, `draw`-voorbereiding, `filelink`/`url`-scans, viewer-`find`;
   - een reproduceerbaar scenario "1 MB output" en "Claude Code TUI";
   - resultaten vastleggen in `docs/PERFORMANCE.md`.
2. **P1:** cache niet wissen bij gelijke snapshot; `modes` in dezelfde lock; buffer hergebruiken.
3. **P3:** terminal- en viewer-zoeken debouncen of naar een worker verplaatsen.
4. **P5:** `GitTick` pauzeren als het venster geen focus heeft; één langlevende worker in plaats van een thread per taak.
5. **T3–T8:** hints uit de keymap; CommandIds voor git/prompt-acties; `is_applicable` bij sneltoetsen; zichtbare melding bij een mislukte sessie-opslag.
6. **`app.rs` opsplitsen** (zie audit §8), gedrag ongewijzigd, met tests vóór en na.
7. **Design tokens** voor typografie, ruimte en radius; daarna thema's (Midnight = huidige Forge Dark, Graphite, Arctic, Cyber, High Contrast) met dezelfde semantische structuur, plus reduced motion.
8. **Docs-drift** wegwerken (ARCHITECTURE-kop, §3, §7, testaantallen, CI).

## P3 — Developer Platform (fase 4)

- **GPU-provider** in `forgeterm-system` (NVML, AMD ADLX, Intel, Linux sysfs) met "niet beschikbaar" als standaard.
- **SSH-profielen en -sessies:** OpenSSH-client als PTY-programma (geen eigen crypto; ssh-agent/keychain) vóór een eigen SFTP-laag.
- **Docker:** optioneel via de `docker`-CLI (detectie zoals agents); containers bekijken, logs, terminal openen; start/stop alleen op klik.
- **Updates:** ondertekende releases (code signing), SHA-256 in release notes, geen automatisch installeren.

## Eerstvolgende stappen

1. Merge-akkoord voor `feature/mission-control`.
2. P0-securityfixes (S1, S3, S2) en T1/T2 op een eigen branch.
3. Milestone 2: Git-dashboard.
