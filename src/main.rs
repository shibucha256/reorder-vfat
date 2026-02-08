use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use ratatui::widgets::*;
use unicode_width::UnicodeWidthStr;

fn main() -> Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen).context("enter alt screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("init terminal")?;

    let result = run_app(&mut terminal);

    disable_raw_mode().ok();
    let mut stdout = io::stdout();
    stdout.execute(LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    result
}

#[derive(Debug, Clone)]
struct Entry {
    name: OsString,
    path: PathBuf,
    is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Renaming,
    ConfirmSort,
    SelectDrive,
}

struct App {
    current_dir: PathBuf,
    entries: Vec<Entry>,
    list_state: ListState,
    drives: Vec<String>,
    drive_state: ListState,
    mode: Mode,
    rename_input: String,
    rename_cursor: usize,
    message: Option<String>,
}

impl App {
    fn new(start_dir: PathBuf) -> Result<Self> {
        let mut app = Self {
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

    fn refresh(&mut self) -> Result<()> {
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

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            self.list_state.select(None);
            return;
        }
        let len = self.entries.len() as isize;
        let current = self.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1);
        self.list_state.select(Some(next as usize));
    }

    fn move_entry(&mut self, delta: isize) {
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

    fn selected_entry(&self) -> Option<&Entry> {
        self.list_state
            .selected()
            .and_then(|idx| self.entries.get(idx))
    }

    fn enter_dir(&mut self) -> Result<()> {
        let Some(entry) = self.selected_entry() else { return Ok(()); };
        if entry.is_dir {
            self.current_dir = entry.path.clone();
            self.list_state.select(Some(0));
            self.refresh()?;
        }
        Ok(())
    }

    fn go_parent(&mut self) -> Result<()> {
        let Some(parent) = self.current_dir.parent() else { return Ok(()); };
        self.current_dir = parent.to_path_buf();
        self.list_state.select(Some(0));
        self.refresh()?;
        Ok(())
    }

    fn start_rename(&mut self) {
        let Some(entry) = self.selected_entry() else { return; };
        let name = entry.name.to_string_lossy().to_string();
        self.mode = Mode::Renaming;
        self.rename_input = name;
        self.rename_cursor = self.rename_input.chars().count();
    }

    fn cancel_rename(&mut self) {
        self.mode = Mode::Normal;
        self.rename_input.clear();
        self.rename_cursor = 0;
        self.message = Some("Rename canceled".to_string());
    }

    fn start_sort_confirm(&mut self) {
        if self.entries.is_empty() {
            self.message = Some("No entries to sort".to_string());
            return;
        }
        self.mode = Mode::ConfirmSort;
    }

    fn start_drive_select(&mut self) -> Result<()> {
        self.drives = list_removable_drives()?;
        if self.drives.is_empty() {
            self.message = Some("No removable drives found".to_string());
            return Ok(());
        }
        self.drive_state.select(Some(0));
        self.mode = Mode::SelectDrive;
        Ok(())
    }

    fn move_drive_selection(&mut self, delta: isize) {
        if self.drives.is_empty() {
            self.drive_state.select(None);
            return;
        }
        let len = self.drives.len() as isize;
        let current = self.drive_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1);
        self.drive_state.select(Some(next as usize));
    }

    fn select_drive(&mut self) -> Result<()> {
        let Some(idx) = self.drive_state.selected() else { return Ok(()); };
        let Some(root) = self.drives.get(idx) else { return Ok(()); };
        self.current_dir = PathBuf::from(root);
        self.list_state.select(Some(0));
        self.mode = Mode::Normal;
        self.refresh()?;
        Ok(())
    }

    fn apply_rename(&mut self) -> Result<()> {
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

fn read_dir_entries(dir: &Path) -> Result<Vec<Entry>> {
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

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let start_dir = std::env::current_dir().context("get current dir")?;
    let mut app = App::new(start_dir)?;

    let tick_rate = Duration::from_millis(200);
    loop {
        terminal.draw(|frame| ui(frame, &mut app))?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match app.mode {
                    Mode::Normal => {
                        if handle_normal_keys(&mut app, key.code, key.modifiers)? {
                            return Ok(());
                        }
                    }
                    Mode::Renaming => {
                        handle_rename_keys(&mut app, key.code)?;
                    }
                    Mode::ConfirmSort => {
                        handle_confirm_sort_keys(&mut app, key.code)?;
                    }
                    Mode::SelectDrive => {
                        handle_drive_select_keys(&mut app, key.code)?;
                    }
                }
            }
        }
    }
}

fn handle_normal_keys(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
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
        KeyCode::Char('L') | KeyCode::Char('l') => app.start_drive_select()?,
        KeyCode::Char('w') => app.start_sort_confirm(),
        KeyCode::Char('s') => {
            app.entries.sort_by(|a, b| a.name.to_string_lossy().cmp(&b.name.to_string_lossy()));
            app.message = Some("Sorted by name".to_string());
        }
        KeyCode::F(5) => app.refresh()?,
        KeyCode::Char('R') if modifiers.contains(KeyModifiers::SHIFT) => app.refresh()?,
        _ => {}
    }
    Ok(false)
}

fn handle_rename_keys(app: &mut App, code: KeyCode) -> Result<()> {
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

fn handle_confirm_sort_keys(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            match perform_vfat_sort(app) {
                Ok(()) => app.message = Some("VFAT order written".to_string()),
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

fn handle_drive_select_keys(app: &mut App, code: KeyCode) -> Result<()> {
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

fn ui(frame: &mut Frame, app: &mut App) {
    let size = frame.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(2)])
        .split(size);

    let title = Paragraph::new(format!(
        "rusttui01  |  {}",
        app.current_dir.to_string_lossy()
    ))
    .style(Style::default().fg(Color::Cyan));
    frame.render_widget(title, chunks[0]);

    match app.mode {
        Mode::SelectDrive => {
            let inner_width = chunks[1].width.saturating_sub(2) as usize;
            let items: Vec<ListItem> = app
                .drives
                .iter()
                .map(|d| ListItem::new(pad_to_width(d, inner_width)))
                .collect();
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Removable Drives"))
                .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
                .highlight_symbol("> ");
            frame.render_stateful_widget(list, chunks[1], &mut app.drive_state);
        }
        _ => {
            let inner_width = chunks[1].width.saturating_sub(2) as usize;
            let items: Vec<ListItem> = app
                .entries
                .iter()
                .map(|e| {
                    let prefix = if e.is_dir { "[D] " } else { "    " };
                    let name = e.name.to_string_lossy();
                    let line = format!("{prefix}{name}");
                    ListItem::new(pad_to_width(&line, inner_width))
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Entries"))
                .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
                .highlight_symbol("> ");

            frame.render_stateful_widget(list, chunks[1], &mut app.list_state);
        }
    }

    match app.mode {
        Mode::Renaming => {
            let area = centered_rect(60, 20, size);
            frame.render_widget(Clear, area);
            let block = Block::default().borders(Borders::ALL).title("Rename");
            let label = "New name: ";
            let inner_width = area.width.saturating_sub(2) as usize;
            let max_width = inner_width.saturating_sub(label.width());
            let (visible, cursor_offset) =
                visible_with_cursor(&app.rename_input, app.rename_cursor, max_width);
            let text = Paragraph::new(format!("{label}{visible}"))
                .block(block)
                .wrap(Wrap { trim: true });
            frame.render_widget(text, area);
            let cursor_x = area.x + 1 + label.width() as u16 + cursor_offset as u16;
            let cursor_y = area.y + 1;
            frame.set_cursor(cursor_x.min(area.x + area.width - 2), cursor_y);
        }
        Mode::ConfirmSort => {
            let area = centered_rect(60, 20, size);
            frame.render_widget(Clear, area);
            let block = Block::default().borders(Borders::ALL).title("Confirm");
            let text = Paragraph::new("Write VFAT order? (y/n)")
                .block(block)
                .wrap(Wrap { trim: true });
            frame.render_widget(text, area);
        }
        _ => {}
    }

    let status = match app.mode {
        Mode::Normal => {
            let msg = app.message.clone().unwrap_or_default();
            format!("↑↓ move  Enter open  Backspace up  r rename  L drives  w write VFAT order  q quit  {msg}")
        }
        Mode::Renaming => format!("Rename to: {}", app.rename_input),
        Mode::ConfirmSort => "Write VFAT order? y/n".to_string(),
        Mode::SelectDrive => "Select drive: Enter open  Esc cancel".to_string(),
    };
    let status_bar = Paragraph::new(status).block(Block::default().borders(Borders::ALL));
    frame.render_widget(status_bar, chunks[2]);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn visible_with_cursor(input: &str, cursor: usize, max_width: usize) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }
    let chars: Vec<char> = input.chars().collect();
    let cursor = cursor.min(chars.len());

    let mut start = 0usize;
    while start < cursor {
        let width_start: usize = chars[start..cursor]
            .iter()
            .map(|c| c.to_string().width())
            .sum();
        if width_start <= max_width.saturating_sub(1) {
            break;
        }
        start += 1;
    }

    let mut end = start;
    let mut width = 0usize;
    while end < chars.len() {
        let w = chars[end].to_string().width();
        if width + w > max_width {
            break;
        }
        width += w;
        end += 1;
    }

    let visible: String = chars[start..end].iter().collect();
    let cursor_offset: usize = chars[start..cursor]
        .iter()
        .map(|c| c.to_string().width())
        .sum();
    (visible, cursor_offset)
}

fn pad_to_width(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width {
        return s.to_string();
    }
    let mut out = String::with_capacity(width);
    out.push_str(s);
    out.push_str(&" ".repeat(width - w));
    out
}

fn perform_vfat_sort(app: &mut App) -> Result<()> {
    ensure_removable_and_not_c(&app.current_dir)?;
    vfat_reorder_dir(&app.current_dir, &app.entries)?;
    Ok(())
}

fn vfat_reorder_dir(dir: &Path, entries: &[Entry]) -> Result<()> {
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

#[cfg(windows)]
fn ensure_removable_and_not_c(path: &Path) -> Result<()> {
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
fn ensure_removable_and_not_c(_path: &Path) -> Result<()> {
    anyhow::bail!("removable drive check is only supported on Windows");
}

#[cfg(windows)]
fn list_removable_drives() -> Result<Vec<String>> {
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
fn list_removable_drives() -> Result<Vec<String>> {
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
