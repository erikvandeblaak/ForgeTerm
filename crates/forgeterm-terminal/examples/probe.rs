//! Diagnostic: run a program in a session and dump the styled cells it produced.
//! Usage: cargo run -p forgeterm-terminal --example probe -- <program> [args...]

use std::time::Duration;

use forgeterm_terminal::{GridSize, Palette, Session, SessionConfig, ShellSpec};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let program = args.next().expect("program");
    let shell = ShellSpec::new("probe", program, args.collect());
    let config = SessionConfig {
        shell: Some(shell),
        size: GridSize::new(120, 30, 8, 16),
        ..Default::default()
    };
    let session = Session::spawn(config, |_| {})?;
    std::thread::sleep(Duration::from_secs(4));

    let palette = Palette::default();
    let snap = session.snapshot(&palette);
    for row in 0..snap.rows {
        let text = snap.row_text(row);
        if text.is_empty() {
            continue;
        }
        let styled: Vec<String> = snap
            .row(row)
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.fg != palette.foreground || c.bg != palette.background || c.flags.bold
            })
            .take(4)
            .map(|(i, c)| {
                format!(
                    "{i}:{:?}/fg{:02x}{:02x}{:02x}/bg{:02x}{:02x}{:02x}{}",
                    c.c,
                    c.fg.r,
                    c.fg.g,
                    c.fg.b,
                    c.bg.r,
                    c.bg.g,
                    c.bg.b,
                    if c.flags.bold { "/B" } else { "" }
                )
            })
            .collect();
        println!("{row:2} {text:<50.50} | {}", styled.join(" "));
    }
    Ok(())
}
