use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use protocol::FileEntry;
use sha2::{Digest, Sha256};

use crate::Plugin;

pub(crate) const SRC_DIR: &str = "src";
pub(crate) const PAGES_DIR: &str = "pages";
pub(crate) const WIDGETS_DIR: &str = "widgets";
pub(crate) const PUBLIC_DIR: &str = "public";
pub(crate) const DEFAULT_PAGE: &str = "app";

#[derive(Debug, Default, Clone)]
pub struct FileManifest {
    pub files: Vec<FileEntry>,
    pub sources: HashMap<String, PathBuf>,
}

impl FileManifest {
    pub fn source(&self, device_path: &str) -> Option<&Path> {
        self.sources.get(device_path).map(PathBuf::as_path)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn safe_join(root: &Path, device_path: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    let mut depth = 0;

    for component in Path::new(device_path).components() {
        match component {
            Component::Normal(part) => {
                out.push(part);
                depth += 1;
            }
            _ => return None,
        }
    }

    (depth >= 2).then_some(out)
}

pub fn device_manifest<'a>(plugins: impl IntoIterator<Item = &'a Plugin>) -> Result<FileManifest> {
    let mut manifest = FileManifest::default();

    for p in plugins {
        let id = &p.manifest.id;
        push_file(&mut manifest, &p.root, &p.root.join("manifest.json"), id)?;
        push_file(&mut manifest, &p.root, &p.pi_js(), id)?;
        push_dir(&mut manifest, &p.root, &p.root.join(SRC_DIR), id)?;
    }

    manifest.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(manifest)
}

pub fn local_manifest(dir: &Path) -> Result<FileManifest> {
    let mut manifest = FileManifest::default();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(manifest),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };

    for entry in entries {
        let root = entry?.path();
        if !root.is_dir() {
            continue;
        }
        let Some(id) = root.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let id = id.to_owned();

        push_optional_file(&mut manifest, &root, &root.join("manifest.json"), &id)?;
        push_optional_file(&mut manifest, &root, &root.join("pi.js"), &id)?;

        let src = root.join(SRC_DIR);
        if src.is_dir() {
            push_dir(&mut manifest, &root, &src, &id)?;
        }
    }

    manifest.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(manifest)
}

fn push_dir(out: &mut FileManifest, root: &Path, dir: &Path, id: &str) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            push_dir(out, root, &path, id)?;
        } else {
            push_file(out, root, &path, id)?;
        }
    }
    Ok(())
}

fn push_file(out: &mut FileManifest, root: &Path, path: &Path, id: &str) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    push_bytes(out, root, path, id, &bytes)
}

fn push_optional_file(out: &mut FileManifest, root: &Path, path: &Path, id: &str) -> Result<()> {
    match fs::read(path) {
        Ok(bytes) => push_bytes(out, root, path, id, &bytes),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn push_bytes(
    out: &mut FileManifest,
    root: &Path,
    path: &Path,
    id: &str,
    bytes: &[u8],
) -> Result<()> {
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("{} is outside {}", path.display(), root.display()))?;

    let mut device_path = String::from(id);
    for part in relative.components() {
        device_path.push('/');
        device_path.push_str(&part.as_os_str().to_string_lossy());
    }

    out.sources.insert(device_path.clone(), path.to_path_buf());
    out.files.push(FileEntry {
        path: device_path,
        sha256: sha256_hex(bytes),
        size_bytes: bytes.len() as u64,
    });
    Ok(())
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub needed: Vec<String>,
    pub extra: Vec<String>,
}

impl SyncPlan {
    pub fn is_empty(&self) -> bool {
        self.needed.is_empty() && self.extra.is_empty()
    }
}

pub fn sync_plan(desired: &[FileEntry], local: &[FileEntry]) -> SyncPlan {
    let have: HashMap<&str, &str> = local
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_str()))
        .collect();

    let wanted: HashSet<&str> = desired.iter().map(|f| f.path.as_str()).collect();

    let needed = desired
        .iter()
        .filter(|f| match have.get(f.path.as_str()) {
            Some(sha256) => *sha256 != f.sha256,
            None => true,
        })
        .map(|f| f.path.clone())
        .collect();

    let extra = local
        .iter()
        .filter(|f| !wanted.contains(f.path.as_str()))
        .map(|f| f.path.clone())
        .collect();

    SyncPlan { needed, extra }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, sha256: &str) -> FileEntry {
        FileEntry {
            path: path.to_owned(),
            sha256: sha256.to_owned(),
            size_bytes: 0,
        }
    }

    #[test]
    fn safe_join_keeps_plain_paths() {
        let root = Path::new("/plugins");
        assert_eq!(
            safe_join(root, "quake/src/pages/app.slint"),
            Some(PathBuf::from("/plugins/quake/src/pages/app.slint"))
        );
    }

    #[test]
    fn safe_join_rejects_escapes() {
        let root = Path::new("/plugins");
        assert_eq!(safe_join(root, "quake/../../etc/passwd"), None);
        assert_eq!(safe_join(root, "/etc/passwd"), None);
        assert_eq!(safe_join(root, "../etc/passwd"), None);
        assert_eq!(safe_join(root, "./quake/pi.js"), None);
        assert_eq!(safe_join(root, "quake"), None);
        assert_eq!(safe_join(root, ""), None);
    }

    #[test]
    fn identical_sides_need_nothing() {
        let files = vec![entry("quake/pi.js", "aaa"), entry("quake/src/pages/app.slint", "bbb")];
        assert!(sync_plan(&files, &files).is_empty());
    }

    #[test]
    fn a_missing_file_is_needed() {
        let desired = vec![entry("quake/pi.js", "aaa"), entry("quake/src/pages/app.slint", "bbb")];
        let local = vec![entry("quake/pi.js", "aaa")];

        let plan = sync_plan(&desired, &local);
        assert_eq!(plan.needed, ["quake/src/pages/app.slint"]);
        assert!(plan.extra.is_empty());
    }

    #[test]
    fn a_changed_file_is_needed_but_not_extra() {
        let desired = vec![entry("quake/pi.js", "new")];
        let local = vec![entry("quake/pi.js", "old")];

        let plan = sync_plan(&desired, &local);
        assert_eq!(plan.needed, ["quake/pi.js"]);
        assert!(plan.extra.is_empty(), "overwriting fixes it, deleting does not");
    }

    #[test]
    fn an_unlisted_file_is_extra() {
        let desired = vec![entry("quake/pi.js", "aaa")];
        let local = vec![entry("quake/pi.js", "aaa"), entry("quake/src/old.slint", "ccc")];

        let plan = sync_plan(&desired, &local);
        assert!(plan.needed.is_empty());
        assert_eq!(plan.extra, ["quake/src/old.slint"]);
    }

    #[test]
    fn a_removed_plugin_is_all_extra() {
        let desired = Vec::new();
        let local = vec![entry("old/manifest.json", "aaa"), entry("old/pi.js", "bbb")];

        let plan = sync_plan(&desired, &local);
        assert!(plan.needed.is_empty());
        assert_eq!(plan.extra, ["old/manifest.json", "old/pi.js"]);
    }

    #[test]
    fn a_bare_device_needs_everything() {
        let desired = vec![entry("quake/pi.js", "aaa"), entry("quake/src/pages/app.slint", "bbb")];

        let plan = sync_plan(&desired, &[]);
        assert_eq!(plan.needed, ["quake/pi.js", "quake/src/pages/app.slint"]);
        assert!(plan.extra.is_empty());
    }
}
