//! What the fixture tests share: the fixture projects on disk.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The fixture projects: every directory of `tests/fixtures` holding
/// `<name>/<name>.exe` (or `.dll`, `.ocx`), sorted by name.
pub fn projects() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut names: Vec<String> = std::fs::read_dir(&root)
        .expect("fixtures directory")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| fixture(name).is_file())
        .collect();
    names.sort();
    names
}

/// The binary of fixture project `name`: its `.exe`, or the `.dll` or
/// `.ocx` of an ActiveX project.
pub fn fixture(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    ["exe", "dll", "ocx"]
        .iter()
        .map(|extension| dir.join(format!("{name}.{extension}")))
        .find(|path| path.is_file())
        .unwrap_or_else(|| dir.join(format!("{name}.exe")))
}
