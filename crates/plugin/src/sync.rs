use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use protocol::FileEntry;
use sha2::{Digest, Sha256};

use crate::Plugin;

const DEVICE_DIRS: &[&str] = &["src", "widgets", "public"];

pub fn device_manifest(plugins: &[Plugin]) -> Result<Vec<FileEntry>> {
    let mut files = Vec::new();

    for p in plugins {
        let id = &p.manifest.id;
        push_file(&mut files, &p.root, &p.root.join("manifest.json"), id)?;
        push_file(&mut files, &p.root, &p.pi_js(), id)?;

        for dir in DEVICE_DIRS {
            let path = p.root.join(dir);
            if path.is_dir() {
                push_dir(&mut files, &p.root, &path, id)?;
            }
        }
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}


fn push_dir(out: &mut Vec<FileEntry>, root: &Path, dir: &Path, id: &str) -> Result<()> {
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

fn push_file(out: &mut Vec<FileEntry>, root: &Path, path: &Path, id: &str) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("{} is outside {}", path.display(), root.display()))?;

    let mut device_path = String::from(id);
    for part in relative.components() {
        device_path.push('/');
        device_path.push_str(&part.as_os_str().to_string_lossy());
    }

    out.push(FileEntry {
        path: device_path,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        size_bytes: bytes.len() as u64,
    });
    Ok(())
}
