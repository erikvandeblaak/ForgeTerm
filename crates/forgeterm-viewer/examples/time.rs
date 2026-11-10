//! Measures loading and highlighting a file: `cargo run -p forgeterm-viewer --example time -- <file>`.

fn main() {
    let path = std::env::args().nth(1).expect("usage: time <file>");
    let t = std::time::Instant::now();
    let doc = forgeterm_viewer::load(std::path::Path::new(&path)).expect("load");
    let loaded = t.elapsed();
    let spans = forgeterm_viewer::highlight(&doc);
    println!(
        "{} lines, {} ({}): load {:?}, highlight {:?} ({})",
        doc.line_count(),
        doc.syntax,
        doc.encoding.label(),
        loaded,
        t.elapsed() - loaded,
        if spans.is_some() { "colored" } else { "plain" }
    );
}
