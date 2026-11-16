//! Emulator, snapshot, link scan and search costs (docs/PERFORMANCE.md).
//! `cargo bench -p forgeterm-terminal`

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use forgeterm_terminal::{Headless, Palette, file_refs, links};

/// About `bytes` of compiler-like output: colored status lines, warnings with
/// paths, long plain lines.
fn build_output(bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes + 200);
    let mut i = 0u32;
    while out.len() < bytes {
        let line = match i % 4 {
            0 => format!(
                "\x1b[1m\x1b[32m   Compiling\x1b[0m crate_{i} v0.{}.{} (C:\\work\\shop\\crates\\crate_{i})\r\n",
                i % 9,
                i % 7
            ),
            1 => format!(
                "\x1b[1m\x1b[33mwarning\x1b[0m: unused variable `x{i}` --> src/module_{}.rs:{}:5\r\n",
                i % 50,
                i % 400 + 1
            ),
            2 => format!(
                "    see https://doc.rust-lang.org/error_codes/E{:04}.html for more, item {i}\r\n",
                i % 800
            ),
            _ => format!("{}\r\n", "lorem ipsum dolor sit amet ".repeat(4)),
        };
        out.extend_from_slice(line.as_bytes());
        i += 1;
    }
    out
}

/// One full redraw of a `cols`×`rows` TUI in truecolor (like Claude Code's
/// interface): every row positioned, a few colored spans per row.
fn tui_frame(cols: usize, rows: usize, n: u32) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("\x1b[?2026h\x1b[H");
    for r in 0..rows {
        out.push_str(&format!("\x1b[{};1H", r + 1));
        let mut col = 0;
        let mut k = 0u32;
        while col < cols {
            let len = (7 + (r as u32 * 3 + k + n) % 13) as usize;
            let len = len.min(cols - col);
            let (cr, cg, cb) = ((r * 9) % 256, (k * 40 + n) % 256, 180);
            out.push_str(&format!("\x1b[38;2;{cr};{cg};{cb}m"));
            if k.is_multiple_of(3) {
                out.push_str("\x1b[1m");
            }
            out.extend(std::iter::repeat_n('a', len.saturating_sub(1)));
            out.push(' ');
            out.push_str("\x1b[0m");
            col += len;
            k += 1;
        }
    }
    out.push_str("\x1b[?2026l");
    out.into_bytes()
}

fn emulator(c: &mut Criterion) {
    let mut g = c.benchmark_group("emulator");
    let output = build_output(1024 * 1024);
    g.throughput(Throughput::Bytes(output.len() as u64));
    g.bench_function("feed_1mb_build_output_120x40", |b| {
        b.iter(|| {
            let mut t = Headless::new(120, 40, 10_000);
            t.feed(black_box(&output));
            t
        })
    });
    let frame = tui_frame(120, 40, 1);
    g.throughput(Throughput::Bytes(frame.len() as u64));
    g.bench_function("feed_tui_frame_120x40", |b| {
        let mut t = Headless::new(120, 40, 1000);
        t.feed(b"\x1b[?1049h");
        b.iter(|| t.feed(black_box(&frame)))
    });
    g.finish();
}

fn snapshot(c: &mut Criterion) {
    let palette = Palette::forge_dark();
    let mut g = c.benchmark_group("snapshot");
    for (cols, rows) in [(120u16, 40u16), (240, 70)] {
        let mut t = Headless::new(cols, rows, 1000);
        t.feed(b"\x1b[?1049h");
        t.feed(&tui_frame(cols as usize, rows as usize, 3));
        g.bench_with_input(
            BenchmarkId::new("tui", format!("{cols}x{rows}")),
            &t,
            |b, t| b.iter(|| t.snapshot(black_box(&palette))),
        );
    }
    // What a repaint after an identical redraw costs now: snapshot + compare.
    let mut t = Headless::new(120, 40, 1000);
    t.feed(b"[?1049h");
    t.feed(&tui_frame(120, 40, 3));
    let shown = t.snapshot(&palette);
    g.bench_function("tui/120x40_unchanged_compare", |b| {
        b.iter(|| t.snapshot(black_box(&palette)) == shown)
    });
    g.finish();
}

fn scans(c: &mut Criterion) {
    // One screen of build output as the renderer sees it (40 rows of text).
    let palette = Palette::forge_dark();
    let mut t = Headless::new(120, 40, 1000);
    t.feed(&build_output(8 * 1024));
    let snap = t.snapshot(&palette);
    let rows: Vec<String> = (0..snap.rows).map(|r| snap.row_text(r)).collect();
    c.bench_function("scan/links_and_paths_screen_120x40", |b| {
        b.iter(|| {
            let mut n = 0;
            for row in &rows {
                n += links(black_box(row)).len();
                n += file_refs(black_box(row)).len();
            }
            n
        })
    });
}

fn search(c: &mut Criterion) {
    let mut t = Headless::new(120, 40, 10_000);
    t.feed(&build_output(1024 * 1024));
    let mut g = c.benchmark_group("search");
    // A query that is not there: the whole scrollback is scanned (each keystroke).
    g.bench_function("scrollback_10k_lines_miss", |b| {
        b.iter(|| t.search(black_box("no-such-text-here"), true))
    });
    g.bench_function("scrollback_10k_lines_hit", |b| {
        b.iter(|| t.search(black_box("Compiling"), true))
    });
    g.finish();
}

criterion_group!(benches, emulator, snapshot, scans, search);
criterion_main!(benches);
