//! UI text must never contain the middle dot "·" (product requirement).

use std::path::Path;

fn scan(dir: &Path, hits: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            scan(&p, hits);
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("rs" | "html" | "js" | "md")) {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            for (i, line) in text.lines().enumerate() {
                if line.contains('\u{00B7}') {
                    hits.push(format!("{}:{}: {}", p.display(), i + 1, line.trim()));
                }
            }
        }
    }
}

#[test]
fn no_middle_dot_in_ui_text() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut hits = Vec::new();
    for d in ["crates/cb-ui/src", "crates/cb-render/src", "crates/cb-shell/src", "crates/cb-remote/src", "crates/cb-update/src", "platforms/windows/src", "platforms/macos/src", "web/remote", "docs/release-notes.md"] {
        let p = root.join(d);
        if p.is_dir() {
            scan(&p, &mut hits);
        } else if p.exists() {
            let text = std::fs::read_to_string(&p).unwrap();
            if text.contains('\u{00B7}') {
                hits.push(p.display().to_string());
            }
        }
    }
    assert!(hits.is_empty(), "middle dot found:\n{}", hits.join("\n"));
}
