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

#[derive(Debug)]
pub(crate) struct SortSummary {
    pub(crate) skipped_system: usize,
    pub(crate) skipped_readonly: usize,
}

pub(crate) fn vfat_reorder_dir(dir: &Path, entries: &[Entry]) -> Result<SortSummary> {
    if entries.is_empty() {
        return Ok(SortSummary {
            skipped_system: 0,
            skipped_readonly: 0,
        });
    }

    let mut sortable: Vec<Entry> = Vec::new();
    let mut skipped_system = 0usize;
    let mut skipped_readonly = 0usize;
    for entry in entries {
        let (is_system, is_readonly) = get_skip_flags(&entry.path)?;
        if is_system || is_readonly {
            if is_system {
                skipped_system += 1;
            }
            if is_readonly {
                skipped_readonly += 1;
            }
            continue;
        }
        sortable.push(entry.clone());
    }

    if sortable.is_empty() {
        return Ok(SortSummary {
            skipped_system,
            skipped_readonly,
        });
    }

    let tmp_dir = unique_tmp_dir(dir)?;
    fs::create_dir(&tmp_dir).with_context(|| format!("create temp dir {:?}", tmp_dir))?;

    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut to_move: Vec<(PathBuf, PathBuf)> = Vec::new();

    for entry in &sortable {
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

    for entry in &sortable {
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

    Ok(SortSummary {
        skipped_system,
        skipped_readonly,
    })
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

#[cfg(windows)]
fn get_file_attributes(path: &Path) -> Result<u32> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetFileAttributesW;

    let mut wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let attrs = unsafe { GetFileAttributesW(wide.as_mut_ptr()) };
    const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFFFFFF;
    if attrs == INVALID_FILE_ATTRIBUTES {
        anyhow::bail!("GetFileAttributesW failed for {:?}", path);
    }
    Ok(attrs)
}

#[cfg(windows)]
fn get_skip_flags(path: &Path) -> Result<(bool, bool)> {
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
    let attrs = get_file_attributes(path)?;
    let is_system = (attrs & FILE_ATTRIBUTE_SYSTEM) != 0;
    let is_readonly = (attrs & FILE_ATTRIBUTE_READONLY) != 0;
    Ok((is_system, is_readonly))
}

#[cfg(not(windows))]
fn get_skip_flags(_path: &Path) -> Result<(bool, bool)> {
    Ok((false, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_file(dir: &Path, name: &str) {
        let path = dir.join(name);
        fs::write(path, b"test").unwrap();
    }

    #[test]
    fn read_dir_entries_reports_files_and_dirs() {
        let dir = tempdir().unwrap();
        create_file(dir.path(), "a.txt");
        fs::create_dir(dir.path().join("sub")).unwrap();

        let entries = read_dir_entries(dir.path()).unwrap();
        assert_eq!(entries.len(), 2);

        let mut names: Vec<String> = entries
            .iter()
            .map(|e| e.name.to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["a.txt".to_string(), "sub".to_string()]);

        let is_dir_map: Vec<(String, bool)> = entries
            .iter()
            .map(|e| (e.name.to_string_lossy().to_string(), e.is_dir))
            .collect();
        assert!(is_dir_map.iter().any(|(n, d)| n == "sub" && *d));
    }

    #[test]
    fn vfat_reorder_dir_preserves_entries_and_cleans_tmp() {
        let dir = tempdir().unwrap();
        create_file(dir.path(), "b.txt");
        create_file(dir.path(), "a.txt");

        let mut entries = read_dir_entries(dir.path()).unwrap();
        entries.sort_by(|a, b| b.name.to_string_lossy().cmp(&a.name.to_string_lossy()));

        let summary = vfat_reorder_dir(dir.path(), &entries).unwrap();
        assert_eq!(summary.skipped_system, 0);
        assert_eq!(summary.skipped_readonly, 0);

        assert!(dir.path().join("a.txt").exists());
        assert!(dir.path().join("b.txt").exists());

        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(".vfatsort_tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn vfat_reorder_dir_rolls_back_on_failure() {
        use std::ffi::OsString;

        let dir = tempdir().unwrap();
        create_file(dir.path(), "a.txt");

        let real = Entry {
            name: OsString::from("a.txt"),
            path: dir.path().join("a.txt"),
            is_dir: false,
        };
        let fake = Entry {
            name: OsString::from("missing.txt"),
            path: dir.path().join("missing.txt"),
            is_dir: false,
        };
        let entries = vec![real, fake];

        let _err = vfat_reorder_dir(dir.path(), &entries).unwrap_err();

        assert!(dir.path().join("a.txt").exists());

        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(".vfatsort_tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn vfat_reorder_dir_skips_system_and_readonly() {
        use std::process::Command;

        let dir = tempdir().unwrap();
        create_file(dir.path(), "keep.txt");
        create_file(dir.path(), "system.txt");
        create_file(dir.path(), "readonly.txt");

        let system_path = dir.path().join("system.txt");
        let readonly_path = dir.path().join("readonly.txt");

        let set_sys = Command::new("attrib")
            .arg(format!("+s"))
            .arg(&system_path)
            .status()
            .expect("attrib +s failed");
        assert!(set_sys.success());
        let set_ro = Command::new("attrib")
            .arg(format!("+r"))
            .arg(&readonly_path)
            .status()
            .expect("attrib +r failed");
        assert!(set_ro.success());

        let mut entries = read_dir_entries(dir.path()).unwrap();
        entries.sort_by(|a, b| a.name.to_string_lossy().cmp(&b.name.to_string_lossy()));

        let summary = vfat_reorder_dir(dir.path(), &entries).unwrap();
        assert_eq!(summary.skipped_system, 1);
        assert_eq!(summary.skipped_readonly, 1);

        assert!(dir.path().join("system.txt").exists());
        assert!(dir.path().join("readonly.txt").exists());
        assert!(dir.path().join("keep.txt").exists());
    }
}
