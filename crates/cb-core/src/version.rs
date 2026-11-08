//! Application version (single source: the workspace Cargo.toml).

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_NAME: &str = "Cloudboard";

/// Major version, e.g. "27".
pub fn major() -> &'static str {
    APP_VERSION.split('.').next().unwrap_or(APP_VERSION)
}

/// Branding shown on the splash screen: "Cloudboard 27".
pub fn splash_title() -> String {
    format!("{} {}", APP_NAME, major())
}

/// Parse "v27.1.3" / "27.1" into comparable parts (pre-release suffix ignored).
pub fn parse(v: &str) -> Option<(u32, u32, u32)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next().map(|s| s.parse().ok()).unwrap_or(Some(0))?;
    let c = it.next().map(|s| s.parse().ok()).unwrap_or(Some(0))?;
    Some((a, b, c))
}

/// True if `candidate` is strictly newer than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_is_27() {
        assert_eq!(APP_VERSION, "27.0.0");
        assert_eq!(splash_title(), "Cloudboard 27");
    }
    #[test]
    fn compare() {
        assert!(is_newer("v27.0.1", "27.0.0"));
        assert!(is_newer("28", "27.9.9"));
        assert!(!is_newer("27.0.0", "27.0.0"));
        assert!(!is_newer("garbage", "27.0.0"));
        assert_eq!(parse("v27.1.0-beta.1"), Some((27, 1, 0)));
    }
}
