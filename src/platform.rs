// reorder-vfat Copyright (c) 2026 shibucha 
// https://github.com/shibucha256/reorder-vfat
// SPDX-License-Identifier: MIT

use anyhow::Result;
use std::ffi::{OsStr, OsString};
use std::path::Path;

pub(crate) trait Platform {
    fn ensure_removable_and_not_c(&self, path: &Path) -> Result<()>;
    fn list_removable_drives(&self) -> Result<Vec<String>>;
    fn is_vfat(&self, path: &Path) -> Result<bool>;
}

pub(crate) struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn ensure_removable_and_not_c(&self, path: &Path) -> Result<()> {
        ensure_removable_and_not_c_impl(path)
    }

    fn list_removable_drives(&self) -> Result<Vec<String>> {
        list_removable_drives_impl()
    }

    fn is_vfat(&self, path: &Path) -> Result<bool> {
        is_vfat_impl(path)
    }
}

#[cfg(windows)]
fn ensure_removable_and_not_c_impl(path: &Path) -> Result<()> {
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
fn ensure_removable_and_not_c_impl(_path: &Path) -> Result<()> {
    anyhow::bail!("removable drive check is only supported on Windows");
}

#[cfg(windows)]
fn list_removable_drives_impl() -> Result<Vec<String>> {
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
fn list_removable_drives_impl() -> Result<Vec<String>> {
    Ok(Vec::new())
}

#[cfg(windows)]
fn is_vfat_impl(path: &Path) -> Result<bool> {
    let root = drive_root(path)?;
    let root_wide = os_str_to_wide(&OsString::from(&root));
    let mut fs_name_buf: [u16; 32] = [0; 32];
    let ok = unsafe {
        get_volume_information_w(
            root_wide.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fs_name_buf.as_mut_ptr(),
            fs_name_buf.len() as u32,
        )
    };
    if ok == 0 {
        anyhow::bail!("GetVolumeInformationW failed for {}", root);
    }

    let fs_name = String::from_utf16_lossy(&fs_name_buf);
    let fs_name = fs_name.trim_end_matches('\u{0}').to_ascii_uppercase();
    Ok(matches!(fs_name.as_str(), "FAT" | "FAT12" | "FAT16" | "FAT32" | "EXFAT"))
}

#[cfg(not(windows))]
fn is_vfat_impl(_path: &Path) -> Result<bool> {
    Ok(false)
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

#[cfg(windows)]
unsafe fn get_volume_information_w(
    root_path: *const u16,
    volume_name: *mut u16,
    volume_name_size: u32,
    serial_number: *mut u32,
    max_component_len: *mut u32,
    fs_flags: *mut u32,
    fs_name: *mut u16,
    fs_name_size: u32,
) -> i32 {
    unsafe extern "system" {
        fn GetVolumeInformationW(
            lpRootPathName: *const u16,
            lpVolumeNameBuffer: *mut u16,
            nVolumeNameSize: u32,
            lpVolumeSerialNumber: *mut u32,
            lpMaximumComponentLength: *mut u32,
            lpFileSystemFlags: *mut u32,
            lpFileSystemNameBuffer: *mut u16,
            nFileSystemNameSize: u32,
        ) -> i32;
    }

    unsafe {
        GetVolumeInformationW(
            root_path,
            volume_name,
            volume_name_size,
            serial_number,
            max_component_len,
            fs_flags,
            fs_name,
            fs_name_size,
        )
    }
}
