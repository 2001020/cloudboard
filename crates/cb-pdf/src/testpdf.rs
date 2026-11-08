//! Tiny PDF writer used by tests and benchmarks (no external tools needed).

/// Build an N-page A4 PDF with text, shapes and many vector paths per page
/// (heavy enough to exercise rendering cost).
pub fn make_pdf(pages: usize, paths_per_page: usize) -> Vec<u8> {
    let mut objs: Vec<String> = Vec::new();
    // 1: catalog, 2: pages, 3: font, then per page: page obj + content obj
    objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    for i in 0..pages {
        let mut c = String::new();
        c.push_str(&format!("BT /F1 36 Tf 72 760 Td (Page {}) Tj ET\n", i + 1));
        c.push_str("BT /F1 12 Tf 72 730 Td (CloudBoard 27 PDF rendering benchmark - the quick brown fox jumps over the lazy dog) Tj ET\n");
        for k in 0..paths_per_page {
            let x = 60.0 + (k % 20) as f32 * 24.0;
            let y = 100.0 + (k / 20 % 25) as f32 * 24.0;
            let r = (k * 37 % 255) as f32 / 255.0;
            c.push_str(&format!("{r:.2} 0.3 0.6 RG 1 w {x} {y} m {} {} l {} {} {} {} {} {} c S\n", x + 10.0, y + 15.0, x + 20.0, y, x + 5.0, y - 10.0, x, y));
        }
        c.push_str("0.9 0.2 0.2 rg 400 60 120 60 re f\n");
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", 5 + i * 2));
        objs.push(format!("<< /Length {} >>\nstream\n{}endstream", c.len(), c));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, o).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", o).as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", objs.len() + 1, xref).as_bytes());
    out
}
