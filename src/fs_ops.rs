use crate::app::Entry;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn read_dir_entries(dir: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir).with_context(|| format!("read_dir {:?}", dir))? {
        let item = item?;
        let path = item.path();
        let name = item.file_name();
        let is_dir = item
            .file_type()
            .map(|ft| ft.is_dir())
            .unwrap_or(false);
        entries.push(Entry { name, path, is_dir });
    }

    Ok(entries)
}

pub(crate) fn vfat_reorder_dir(dir: &Path, entries: &[Entry]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let tmp_dir = unique_tmp_dir(dir)?;
    fs::create_dir(&tmp_dir).with_context(|| format!("create temp dir {:?}", tmp_dir))?;

    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut to_move: Vec<(PathBuf, PathBuf)> = Vec::new();

    for entry in entries {
        let from = entry.path.clone();
        let to = tmp_dir.join(&entry.name);
        to_move.push((from, to));
    }

    for (from, to) in &to_move {
        if let Err(err) = fs::rename(from, to) {
            for (moved_from, moved_to) in moved.iter().rev() {
                let _ = fs::rename(moved_to, moved_from);
            }
            let _ = fs::remove_dir(&tmp_dir);
            return Err(err).with_context(|| "move to temp dir failed");
        }
        moved.push((from.clone(), to.clone()));
    }

    for entry in entries {
        let from = tmp_dir.join(&entry.name);
        let to = dir.join(&entry.name);
        if let Err(err) = fs::rename(&from, &to) {
            if let Ok(remaining) = fs::read_dir(&tmp_dir) {
                for rem in remaining.flatten() {
                    let name = rem.file_name();
                    let from_left = tmp_dir.join(&name);
                    let to_left = dir.join(&name);
                    let _ = fs::rename(&from_left, &to_left);
                }
            }
            let _ = fs::remove_dir(&tmp_dir);
            return Err(err).with_context(|| "move back from temp dir failed");
        }
    }

    let _ = fs::remove_dir(&tmp_dir);

    Ok(())
}

fn unique_tmp_dir(dir: &Path) -> Result<PathBuf> {
    let base = dir.join(".vfatsort_tmp");
    if !base.exists() {
        return Ok(base);
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    for i in 0..1000 {
        let candidate = dir.join(format!(".vfatsort_tmp_{stamp}_{i}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    anyhow::bail!("could not create unique temp dir");
}
