//! Windows: gives forgeterm.exe its icon (Explorer, shortcuts, taskbar pins).
//! The .ico is rendered from assets/branding/forgeterm-logo.svg at build time
//! (PNG images in an ICO container, supported since Windows Vista) and embedded
//! with the Windows SDK resource compiler. Other targets: nothing to do.

use std::path::PathBuf;

const SIZES: [u32; 5] = [16, 24, 32, 48, 256];

fn main() {
    let logo = PathBuf::from("../../assets/branding/forgeterm-logo.svg");
    println!("cargo:rerun-if-changed={}", logo.display());
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let svg = std::fs::read(&logo).expect("read the logo");
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())
        .expect("parse the logo");

    let mut images = Vec::new();
    for size in SIZES {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("pixmap");
        let scale = size as f32 / tree.size().width().max(tree.size().height());
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        images.push((size, pixmap.encode_png().expect("encode png")));
    }

    // ICONDIR + ICONDIRENTRY per image + the PNG data.
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, png) in &images {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &images {
        ico.extend_from_slice(png);
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let ico_path = out.join("forgeterm.ico");
    std::fs::write(&ico_path, ico).expect("write icon");
    let rc_path = out.join("forgeterm.rc");
    // Resource id 1: Explorer uses the first icon resource of an executable.
    let rc = format!(
        "1 ICON \"{}\"\n",
        ico_path.display().to_string().replace('\\', "\\\\")
    );
    std::fs::write(&rc_path, rc).expect("write rc");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("compile the icon resource (Windows SDK rc.exe)");
}
