use anyhow::Result;
use std::ffi::{OsStr, OsString};
use std::path::Path;

#[cfg(windows)]
pub(crate) fn ensure_removable_and_not_c(path: &Path) -> Result<()> {
    let root = drive_root(path)?;
    let letter = root
        .chars()
        .next()
        .unwrap_or('C')
        .to_ascii_uppercase();
    if letter == 'C' {
        anyhow::bail!("C: drive is not allowed");
    }

    let wide = os_str_to_wide(&OsString::from(root));
    let drive_type = unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(wide.as_ptr()) };
    const DRIVE_REMOVABLE: u32 = 2;
    if drive_type != DRIVE_REMOVABLE {
        anyhow::bail!("target drive is not removable");
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn ensure_removable_and_not_c(_path: &Path) -> Result<()> {
    anyhow::bail!("removable drive check is only supported on Windows");
}

#[cfg(windows)]
pub(crate) fn list_removable_drives() -> Result<Vec<String>> {
    use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        anyhow::bail!("GetLogicalDrives failed");
    }
    let mut drives = Vec::new();
    for i in 0..26 {
        if (mask & (1 << i)) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let wide = os_str_to_wide(&OsString::from(&root));
        let drive_type =
            unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(wide.as_ptr()) };
        const DRIVE_REMOVABLE: u32 = 2;
        if drive_type == DRIVE_REMOVABLE {
            drives.push(root);
        }
    }
    Ok(drives)
}

#[cfg(not(windows))]
pub(crate) fn list_removable_drives() -> Result<Vec<String>> {
    Ok(Vec::new())
}

#[cfg(windows)]
fn drive_root(path: &Path) -> Result<String> {
    use std::path::Component;
    use std::path::Prefix;
    for comp in path.components() {
        if let Component::Prefix(prefix) = comp {
            match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    let ch = (letter as char).to_ascii_uppercase();
                    return Ok(format!("{ch}:\\"));
                }
                _ => {}
            }
        }
    }
    anyhow::bail!("could not determine drive root");
}

#[cfg(windows)]
fn os_str_to_wide(s: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let mut wide: Vec<u16> = s.encode_wide().collect();
    wide.push(0);
    wide
}
