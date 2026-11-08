//! Open Packaging Conventions: the zip container and part relationships.

use std::collections::HashMap;
use std::io::Read;

pub struct Package {
    parts: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Rel {
    pub kind: String,
    /// Absolute part name (no leading slash), or the raw target if external.
    pub target: String,
    pub external: bool,
}

/// Largest total size we unpack (guards against zip bombs).
const MAX_TOTAL: u64 = 1 << 30;

impl Package {
    pub fn open(bytes: &[u8]) -> Result<Package, String> {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "不是有效的 Office 文件（无法解压）".to_string())?;
        let mut parts = HashMap::new();
        let mut total = 0u64;
        for i in 0..z.len() {
            let mut f = z.by_index(i).map_err(|e| e.to_string())?;
            if f.is_dir() {
                continue;
            }
            total += f.size();
            if total > MAX_TOTAL {
                return Err("文件过大".into());
            }
            let name = f.name().trim_start_matches('/').to_string();
            let mut v = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut v).map_err(|e| e.to_string())?;
            parts.insert(name, v);
        }
        Ok(Package { parts })
    }

    pub fn bytes(&self, name: &str) -> Option<&[u8]> {
        self.parts.get(name.trim_start_matches('/')).map(|v| v.as_slice())
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        let b = self.bytes(name)?;
        let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
        std::str::from_utf8(b).ok()
    }

    pub fn has(&self, name: &str) -> bool {
        self.parts.contains_key(name.trim_start_matches('/'))
    }

    /// Relationships of `part` keyed by id.
    pub fn rels(&self, part: &str) -> HashMap<String, Rel> {
        let (dir, file) = match part.rfind('/') {
            Some(i) => (&part[..i], &part[i + 1..]),
            None => ("", part),
        };
        let rels_name = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
        let mut out = HashMap::new();
        let Some(xml) = self.text(&rels_name) else { return out };
        let Ok(doc) = roxmltree::Document::parse(xml) else { return out };
        for r in doc.root_element().children().filter(|n| n.has_tag_name_local("Relationship")) {
            let (Some(id), Some(target)) = (r.attribute("Id"), r.attribute("Target")) else { continue };
            let external = r.attribute("TargetMode") == Some("External");
            let kind = r.attribute("Type").unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
            let target = if external { target.to_string() } else { resolve(dir, target) };
            out.insert(id.to_string(), Rel { kind, target, external });
        }
        out
    }

    /// First relationship target of the given type (last path segment of the type URI).
    pub fn rel_of_kind(&self, part: &str, kind: &str) -> Option<String> {
        self.rels(part).into_values().find(|r| r.kind == kind && !r.external).map(|r| r.target)
    }
}

/// Resolve a relative target against the directory of the source part.
pub fn resolve(dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut segs: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
    for s in target.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    segs.join("/")
}

/// Local-name helpers for roxmltree (OOXML prefixes vary between producers).
pub trait NodeExt<'a, 'i> {
    fn has_tag_name_local(&self, name: &str) -> bool;
    fn child(&self, name: &str) -> Option<roxmltree::Node<'a, 'i>>;
    fn children_named(&self, name: &'static str) -> Box<dyn Iterator<Item = roxmltree::Node<'a, 'i>> + 'a>;
    fn path(&self, names: &[&str]) -> Option<roxmltree::Node<'a, 'i>>;
    fn attr(&self, name: &str) -> Option<&'a str>;
    fn attr_i64(&self, name: &str) -> Option<i64>;
    fn attr_f32(&self, name: &str) -> Option<f32>;
    fn attr_bool(&self, name: &str) -> Option<bool>;
}

impl<'a, 'i: 'a> NodeExt<'a, 'i> for roxmltree::Node<'a, 'i> {
    fn has_tag_name_local(&self, name: &str) -> bool {
        self.is_element() && self.tag_name().name() == name
    }
    fn child(&self, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
        self.children().find(|c| c.is_element() && c.tag_name().name() == name)
    }
    fn children_named(&self, name: &'static str) -> Box<dyn Iterator<Item = roxmltree::Node<'a, 'i>> + 'a> {
        Box::new(self.children().filter(move |c| c.is_element() && c.tag_name().name() == name))
    }
    fn path(&self, names: &[&str]) -> Option<roxmltree::Node<'a, 'i>> {
        let mut n = *self;
        for name in names {
            n = n.child(name)?;
        }
        Some(n)
    }
    /// Attribute by local name (ignores namespace prefixes like r:id / w:val).
    fn attr(&self, name: &str) -> Option<&'a str> {
        self.attributes().find(|a| a.name() == name).map(|a| a.value())
    }
    fn attr_i64(&self, name: &str) -> Option<i64> {
        self.attr(name).and_then(|v| v.trim().parse::<f64>().ok()).map(|v| v as i64)
    }
    fn attr_f32(&self, name: &str) -> Option<f32> {
        self.attr(name).and_then(|v| v.trim().trim_end_matches('%').parse::<f32>().ok())
    }
    fn attr_bool(&self, name: &str) -> Option<bool> {
        self.attr(name).map(|v| matches!(v, "1" | "true" | "on"))
    }
}

/// EMU (English Metric Units) per point.
pub const EMU_PER_PT: f32 = 12700.0;

pub fn emu(v: i64) -> f32 {
    v as f32 / EMU_PER_PT
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_targets() {
        assert_eq!(resolve("ppt/slides", "../media/image1.png"), "ppt/media/image1.png");
        assert_eq!(resolve("ppt", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("", "ppt/presentation.xml"), "ppt/presentation.xml");
        assert_eq!(resolve("word", "/word/media/a.png"), "word/media/a.png");
    }
}
