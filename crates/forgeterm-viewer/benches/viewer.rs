//! Viewer search cost (docs/PERFORMANCE.md). `cargo bench -p forgeterm-viewer`

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

fn find(c: &mut Criterion) {
    // A file near the viewer's 8 MB limit: ~100k lines of code-like text.
    let dir = std::env::temp_dir().join(format!("forgeterm-viewer-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.rs");
    let mut text = String::new();
    let mut i = 0;
    while text.len() < 7 * 1024 * 1024 + 512 * 1024 {
        text.push_str(&format!(
            "    let value_{i} = compute(input_{i}, Options {{ depth: {}, verbose: false }}); // step {i}\n",
            i % 17
        ));
        i += 1;
    }
    std::fs::write(&path, &text).unwrap();
    let doc = forgeterm_viewer::load(&path).unwrap();

    let mut g = c.benchmark_group("viewer_find_7_5mb");
    g.sample_size(20);
    g.bench_function("miss", |b| {
        b.iter(|| forgeterm_viewer::find(&doc, black_box("no_such_identifier")))
    });
    g.bench_function("hit_every_line", |b| {
        b.iter(|| forgeterm_viewer::find(&doc, black_box("compute")))
    });
    g.finish();
    let _ = std::fs::remove_dir_all(&dir);
}

criterion_group!(benches, find);
criterion_main!(benches);
