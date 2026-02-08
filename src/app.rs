// reorder-vfat Copyright (c) 2026 shibucha 
// https://github.com/shibucha256/reorder-vfat
// SPDX-License-Identifier: MIT

use crate::fs_ops::{read_dir_entries, vfat_reorder_dir, SortSummary};
use crate::platform::Platform;
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::widgets::ListState;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) name: OsString,
    pub(crate) path: PathBuf,
    pub(crate) is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Normal,
    Renaming,
    ConfirmSort,
    SelectDrive,
}

pub(crate) struct App<P: Platform> {
    pub(crate) platform: P,
    pub(crate) current_dir: PathBuf,
    pub(crate) entries: Vec<Entry>,
    pub(crate) list_state: ListState,
    pub(crate) drives: Vec<String>,
    pub(crate) drive_state: ListState,
    pub(crate) mode: Mode,
    pub(crate) rename_input: String,
    pub(crate) rename_cursor: usize,
    pub(crate) message: Option<String>,
}

impl<P: Platform> App<P> {
    pub(crate) fn new(start_dir: PathBuf, platform: P) -> Result<Self> {
        let mut app = Self {
            platform,
            current_dir: start_dir,
            entries: Vec::new(),
            list_state: ListState::default(),
            drives: Vec::new(),
            drive_state: ListState::default(),
            mode: Mode::Normal,
            rename_input: String::new(),
            rename_cursor: 0,
            message: None,
        };
        app.refresh()?;
        Ok(app)
    }

    pub(crate) fn refresh(&mut self) -> Result<()> {
        self.entries = read_dir_entries(&self.current_dir)?;
        if self.entries.is_empty() {
            self.list_state.select(None);
        } else if self.list_state.selected().is_none() {
            self.list_state.select(Some(0));
        } else {
            let idx = self.list_state.selected().unwrap();
            let capped = idx.min(self.entries.len() - 1);
            self.list_state.select(Some(capped));
        }
        Ok(())
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            self.list_state.select(None);
            return;
        }
        let len = self.entries.len() as isize;
        let current = self.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1);
        self.list_state.select(Some(next as usize));
    }

    pub(crate) fn move_entry(&mut self, delta: isize) {
        if self.entries.is_empty() {
            self.list_state.select(None);
            return;
        }
        let len = self.entries.len() as isize;
        let current = self.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1);
        if current == next {
            return;
        }
        let a = current as usize;
        let b = next as usize;
        self.entries.swap(a, b);
        self.list_state.select(Some(b));
    }

    pub(crate) fn selected_entry(&self) -> Option<&Entry> {
        self.list_state
            .selected()
            .and_then(|idx| self.entries.get(idx))
    }

    pub(crate) fn enter_dir(&mut self) -> Result<()> {
        let Some(entry) = self.selected_entry() else { return Ok(()); };
        if entry.is_dir {
            self.current_dir = entry.path.clone();
            self.list_state.select(Some(0));
            self.refresh()?;
        }
        Ok(())
    }

    pub(crate) fn go_parent(&mut self) -> Result<()> {
        let Some(parent) = self.current_dir.parent() else { return Ok(()); };
        self.current_dir = parent.to_path_buf();
        self.list_state.select(Some(0));
        self.refresh()?;
        Ok(())
    }

    pub(crate) fn start_rename(&mut self) {
        let Some(entry) = self.selected_entry() else { return; };
        let name = entry.name.to_string_lossy().to_string();
        self.mode = Mode::Renaming;
        self.rename_input = name;
        self.rename_cursor = self.rename_input.chars().count();
    }

    pub(crate) fn cancel_rename(&mut self) {
        self.mode = Mode::Normal;
        self.rename_input.clear();
        self.rename_cursor = 0;
        self.message = Some("Rename canceled".to_string());
    }

    pub(crate) fn start_sort_confirm(&mut self) {
        if self.entries.is_empty() {
            self.message = Some("No entries to sort".to_string());
            return;
        }
        self.mode = Mode::ConfirmSort;
    }

    pub(crate) fn start_drive_select(&mut self) -> Result<()> {
        self.drives = self.platform.list_removable_drives()?;
        if self.drives.is_empty() {
            self.message = Some("No removable drives found".to_string());
            return Ok(());
        }
        self.drive_state.select(Some(0));
        self.mode = Mode::SelectDrive;
        Ok(())
    }

    pub(crate) fn move_drive_selection(&mut self, delta: isize) {
        if self.drives.is_empty() {
            self.drive_state.select(None);
            return;
        }
        let len = self.drives.len() as isize;
        let current = self.drive_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1);
        self.drive_state.select(Some(next as usize));
    }

    pub(crate) fn select_drive(&mut self) -> Result<()> {
        let Some(idx) = self.drive_state.selected() else { return Ok(()); };
        let Some(root) = self.drives.get(idx) else { return Ok(()); };
        self.current_dir = PathBuf::from(root);
        self.list_state.select(Some(0));
        self.mode = Mode::Normal;
        self.refresh()?;
        Ok(())
    }

    pub(crate) fn apply_rename(&mut self) -> Result<()> {
        let Some(entry) = self.selected_entry() else { return Ok(()); };
        let new_name = self.rename_input.trim();
        if new_name.is_empty() {
            self.message = Some("Rename failed: empty name".to_string());
            self.mode = Mode::Normal;
            return Ok(());
        }

        let new_path = entry.path.parent().unwrap_or(Path::new(".")).join(new_name);
        fs::rename(&entry.path, &new_path).with_context(|| {
            format!(
                "rename {:?} -> {:?}",
                entry.path.to_string_lossy(),
                new_path.to_string_lossy()
            )
        })?;
        self.message = Some("Renamed".to_string());
        self.mode = Mode::Normal;
        self.rename_input.clear();
        self.rename_cursor = 0;
        self.refresh()?;
        Ok(())
    }
}

pub(crate) fn handle_normal_keys<P: Platform>(
    app: &mut App<P>,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<bool> {
    match code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Up if modifiers.contains(KeyModifiers::CONTROL) => app.move_entry(-1),
        KeyCode::Down if modifiers.contains(KeyModifiers::CONTROL) => app.move_entry(1),
        KeyCode::Insert => app.move_entry(1),
        KeyCode::Delete => app.move_entry(-1),
        KeyCode::Up => app.move_selection(-1),
        KeyCode::Down => app.move_selection(1),
        KeyCode::Enter => app.enter_dir()?,
        KeyCode::Backspace => app.go_parent()?,
        KeyCode::Char('r') => app.start_rename(),
        KeyCode::Char('l') => app.start_drive_select()?,
        KeyCode::Char('w') => app.start_sort_confirm(),
        KeyCode::Char('s') => {
            app.entries
                .sort_by(|a, b| a.name.to_string_lossy().cmp(&b.name.to_string_lossy()));
            app.message = Some("Sorted by name".to_string());
        }
        KeyCode::F(5) => app.refresh()?,
        KeyCode::Char('R') if modifiers.contains(KeyModifiers::SHIFT) => app.refresh()?,
        _ => {}
    }
    Ok(false)
}

pub(crate) fn handle_rename_keys<P: Platform>(app: &mut App<P>, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Esc => app.cancel_rename(),
        KeyCode::Enter => app.apply_rename()?,
        KeyCode::Backspace => {
            if app.rename_cursor > 0 {
                let new_cursor = app.rename_cursor - 1;
                let mut chars: Vec<char> = app.rename_input.chars().collect();
                chars.remove(new_cursor);
                app.rename_input = chars.iter().collect();
                app.rename_cursor = new_cursor;
            }
        }
        KeyCode::Delete => {
            let mut chars: Vec<char> = app.rename_input.chars().collect();
            if app.rename_cursor < chars.len() {
                chars.remove(app.rename_cursor);
                app.rename_input = chars.iter().collect();
            }
        }
        KeyCode::Left => {
            if app.rename_cursor > 0 {
                app.rename_cursor -= 1;
            }
        }
        KeyCode::Right => {
            let len = app.rename_input.chars().count();
            if app.rename_cursor < len {
                app.rename_cursor += 1;
            }
        }
        KeyCode::Home => app.rename_cursor = 0,
        KeyCode::End => app.rename_cursor = app.rename_input.chars().count(),
        KeyCode::Char(ch) => {
            let mut chars: Vec<char> = app.rename_input.chars().collect();
            chars.insert(app.rename_cursor, ch);
            app.rename_input = chars.iter().collect();
            app.rename_cursor += 1;
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn handle_confirm_sort_keys<P: Platform>(app: &mut App<P>, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            match perform_vfat_sort(app) {
                Ok(summary) => {
                    if summary.skipped_system == 0 && summary.skipped_readonly == 0 {
                        app.message = Some("VFAT order written".to_string());
                    } else {
                        app.message = Some(format!(
                            "VFAT order written (skipped system: {}, readonly: {})",
                            summary.skipped_system, summary.skipped_readonly
                        ));
                    }
                }
                Err(err) => app.message = Some(format!("Sort failed: {err}")),
            }
            app.mode = Mode::Normal;
            app.refresh()?;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.message = Some("Sort canceled".to_string());
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn handle_drive_select_keys<P: Platform>(app: &mut App<P>, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Up => app.move_drive_selection(-1),
        KeyCode::Down => app.move_drive_selection(1),
        KeyCode::Enter => app.select_drive()?,
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.message = Some("Drive selection canceled".to_string());
        }
        _ => {}
    }
    Ok(())
}

fn perform_vfat_sort<P: Platform>(app: &mut App<P>) -> Result<SortSummary> {
    app.platform.ensure_removable_and_not_c(&app.current_dir)?;
    if !app.platform.is_vfat(&app.current_dir)? {
        anyhow::bail!("target drive is not FAT/exFAT");
    }
    vfat_reorder_dir(&app.current_dir, &app.entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;
    use crate::platform::Platform;

    #[derive(Clone, Copy)]
    struct FakePlatform;

    impl Platform for FakePlatform {
        fn ensure_removable_and_not_c(&self, _path: &Path) -> Result<()> {
            Ok(())
        }

        fn list_removable_drives(&self) -> Result<Vec<String>> {
            Ok(vec!["E:\\".to_string()])
        }

        fn is_vfat(&self, _path: &Path) -> Result<bool> {
            Ok(true)
        }
    }

    fn create_file(dir: &Path, name: &str) {
        let path = dir.join(name);
        fs::write(path, b"test").unwrap();
    }

    #[test]
    fn move_selection_clamps() {
        let dir = tempdir().unwrap();
        create_file(dir.path(), "a.txt");
        create_file(dir.path(), "b.txt");
        let mut app = App::new(dir.path().to_path_buf(), FakePlatform).unwrap();

        app.move_selection(-10);
        assert_eq!(app.list_state.selected(), Some(0));

        app.move_selection(10);
        assert_eq!(app.list_state.selected(), Some(app.entries.len() - 1));
    }

    #[test]
    fn move_entry_swaps_and_updates_selection() {
        let dir = tempdir().unwrap();
        create_file(dir.path(), "a.txt");
        create_file(dir.path(), "b.txt");
        let mut app = App::new(dir.path().to_path_buf(), FakePlatform).unwrap();

        app.list_state.select(Some(0));
        let first_before = app.entries[0].name.clone();
        let second_before = app.entries[1].name.clone();

        app.move_entry(1);

        assert_eq!(app.entries[0].name, second_before);
        assert_eq!(app.entries[1].name, first_before);
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn rename_updates_filesystem_and_state() {
        let dir = tempdir().unwrap();
        create_file(dir.path(), "old.txt");
        let mut app = App::new(dir.path().to_path_buf(), FakePlatform).unwrap();

        app.list_state.select(Some(0));
        app.start_rename();
        app.rename_input = "new.txt".to_string();
        app.rename_cursor = app.rename_input.len();
        app.apply_rename().unwrap();

        assert!(dir.path().join("new.txt").exists());
        assert!(!dir.path().join("old.txt").exists());
        assert_eq!(app.message.as_deref(), Some("Renamed"));
        assert_eq!(app.mode, Mode::Normal);
    }
}
