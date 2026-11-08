//! Render every page of a .pptx/.docx to PNG files.
//!   cargo run --release -p cb-office --example render -- in.pptx out-dir [width-px]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (input, out) = (&a[1], std::path::PathBuf::from(&a[2]));
    let width: f32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(1280.0);
    std::fs::create_dir_all(&out).unwrap();
    let bytes = std::fs::read(input).unwrap();
    let fmt = cb_office::format_for(input).expect("unsupported");
    let mut fs = cosmic_text::FontSystem::new();
    let mut map = cb_office::FontMap::new();
    let t0 = std::time::Instant::now();
    let doc = cb_office::load(&bytes, fmt, &mut fs, &mut map).unwrap();
    println!("loaded {} pages in {:?}", doc.pages.len(), t0.elapsed());
    let mut r = cb_office::Renderer::new();
    for (i, p) in doc.pages.iter().enumerate() {
        let t = std::time::Instant::now();
        let pm = cb_office::render_page(&mut r, &mut fs, p, width / p.width).unwrap();
        pm.save_png(out.join(format!("page-{:02}.png", i + 1))).unwrap();
        println!("page {} ({} items, {} steps) rendered in {:?}", i + 1, p.items.len(), p.steps.len(), t.elapsed());
    }
}
