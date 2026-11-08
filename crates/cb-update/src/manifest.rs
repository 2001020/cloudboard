//! `latest.json` manifest attached to every GitHub release.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetInfo {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub min_supported: String,
    #[serde(default)]
    pub mandatory: bool,
    #[serde(default)]
    pub published: String,
    /// Keyed by platform: "windows-x64", "windows-arm64", "macos-universal".
    pub assets: BTreeMap<String, AssetInfo>,
}
