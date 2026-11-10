# ForgeTerm — Test Plan

## Automatisch (`cargo test --workspace`)

| Test | Wat | Status Win11 |
|---|---|---|
| `input::tests` (7) | Toets-encoding (ctrl, alt, DECCKM, modifiers, F-toetsen), muisrapporten SGR/legacy, paste-veiligheid (geen auto-submit, ESC-injectie) | ✅ |
| `palette::tests` (2) | 256-kleurenkubus, grijstinten, speciale kleuren | ✅ |
| `tests/pty_roundtrip.rs` | Echte shell (cmd / sh) in PTY: commando in, resultaat in grid, resize, exit-event | ✅ |

Kwaliteitspoort voor iedere wijziging:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

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
| G13 | Plak 1 regel + newline | Ingevoegd, níet uitgevoerd | ✅ |
| G14 | `exit` → Enter | "Exited (0)", Enter herstart shell | ✅ |
| G15 | Venster sluiten | Geen weesprocessen (pwsh/conhost) | ✅ |

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
