use crate::cli::{parse_args, CliAction, HELP};
use crate::markdown::{render_markdown, total_rows};
use crate::rendered::{plain_line, RenderLine};
use crate::scroll::{clamp_offset, page_step, scroll_by};
use crate::search::{find_matches, first_at_or_after, next_index, previous_index};
use crate::selection::SelectionRange;
use crate::selection::{selected_text, SelectionPoint, SelectionState};
use crate::terminal::Terminal;
use crate::watcher::{absolute_path, FileWatcher};
use crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const RELOAD_DEBOUNCE: Duration = Duration::from_millis(90);
const POLL_TICK: Duration = Duration::from_millis(40);
const WHEEL_LINES: isize = 3;

#[derive(Debug)]
pub enum AppError {
    Cli(String),
    Io(io::Error),
    Notify(notify::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Notify(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<io::Error> for AppError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<notify::Error> for AppError {
    fn from(err: notify::Error) -> Self {
        Self::Notify(err)
    }
}

pub fn run() -> Result<(), AppError> {
    match parse_args(env::args_os()).map_err(AppError::Cli)? {
        CliAction::Help => {
            println!("{HELP}");
            Ok(())
        }
        CliAction::Version => {
            println!("mdview {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CliAction::Run(path) => run_file(path),
    }
}

fn run_file(path: PathBuf) -> Result<(), AppError> {
    let path = absolute_path(&path);
    let mut terminal = Terminal::enter()?;
    let (mut viewport_cols, mut viewport_rows) = terminal.viewport_size()?;
    let mut state = ViewerState::load(path, viewport_cols)?;
    let mut watcher = FileWatcher::new(&state.path)?;

    terminal.draw(
        &state.lines,
        state.scroll,
        &state.status(viewport_rows as usize, None),
        state.selection_range(),
        state.search_highlights(),
    )?;

    let mut reload_due: Option<Instant> = None;
    let mut draw_needed = false;

    loop {
        if event::poll(POLL_TICK)? {
            match event::read()? {
                TermEvent::Key(key) => {
                    if is_control_c(key) {
                        state.copy_selection(&mut terminal, viewport_cols as usize)?;
                        draw_needed = true;
                    } else if state.search_input_active() {
                        match state.handle_search_input(key, viewport_rows as usize) {
                            SearchKeyResult::Redraw => draw_needed = true,
                            SearchKeyResult::None => {}
                        }
                    } else {
                        match input_action(key) {
                            InputAction::Quit => break,
                            InputAction::Scroll(delta) => {
                                state.scroll_by(delta, viewport_rows as usize);
                                draw_needed = true;
                            }
                            InputAction::PageUp => {
                                state.scroll_by(
                                    -(page_step(viewport_rows as usize) as isize),
                                    viewport_rows as usize,
                                );
                                draw_needed = true;
                            }
                            InputAction::PageDown => {
                                state.scroll_by(
                                    page_step(viewport_rows as usize) as isize,
                                    viewport_rows as usize,
                                );
                                draw_needed = true;
                            }
                            InputAction::Top => {
                                state.scroll = 0;
                                draw_needed = true;
                            }
                            InputAction::Bottom => {
                                state.scroll = state.max_scroll(viewport_rows as usize);
                                draw_needed = true;
                            }
                            InputAction::Copy => {
                                state.copy_selection(&mut terminal, viewport_cols as usize)?;
                                draw_needed = true;
                            }
                            InputAction::Search => {
                                state.begin_search_input();
                                draw_needed = true;
                            }
                            InputAction::NextSearch => {
                                state.next_search_match(viewport_rows as usize);
                                draw_needed = true;
                            }
                            InputAction::PreviousSearch => {
                                state.previous_search_match(viewport_rows as usize);
                                draw_needed = true;
                            }
                            InputAction::None => {}
                        }
                    }
                }
                TermEvent::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp if terminal.mouse_mode().handles_wheel() => {
                        state.scroll_by(-WHEEL_LINES, viewport_rows as usize);
                        draw_needed = true;
                    }
                    MouseEventKind::ScrollDown if terminal.mouse_mode().handles_wheel() => {
                        state.scroll_by(WHEEL_LINES, viewport_rows as usize);
                        draw_needed = true;
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if terminal.mouse_mode().handles_selection() =>
                    {
                        if let Some(point) = state.mouse_point(mouse, viewport_rows as usize) {
                            state.selection.begin(point);
                            state.last_status = Some("selecting".to_string());
                            draw_needed = true;
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left)
                        if terminal.mouse_mode().handles_selection()
                            && state.selection.is_active() =>
                    {
                        if let Some(point) = state.mouse_point(mouse, viewport_rows as usize) {
                            state.selection.update(point);
                            state.last_status = state
                                .selection_status(viewport_cols as usize)
                                .or_else(|| Some("selecting".to_string()));
                            draw_needed = true;
                        }
                    }
                    MouseEventKind::Up(MouseButton::Left)
                        if terminal.mouse_mode().handles_selection()
                            && state.selection.is_active() =>
                    {
                        if let Some(point) = state.mouse_point(mouse, viewport_rows as usize) {
                            state.selection.finish(point);
                            state.last_status = state
                                .selection_status(viewport_cols as usize)
                                .or_else(|| Some("no selection".to_string()));
                        } else {
                            state.selection.clear();
                            state.last_status = Some("selection cleared".to_string());
                        }
                        draw_needed = true;
                    }
                    MouseEventKind::Down(MouseButton::Right)
                        if terminal.mouse_mode().handles_selection() =>
                    {
                        state.copy_selection(&mut terminal, viewport_cols as usize)?;
                        draw_needed = true;
                    }
                    _ => {}
                },
                TermEvent::Resize(cols, rows) => {
                    viewport_cols = cols.max(1);
                    viewport_rows = rows.saturating_sub(1).max(1);
                    state.render(viewport_cols);
                    state.scroll = clamp_offset(
                        state.scroll,
                        total_rows(&state.lines),
                        viewport_rows as usize,
                    );
                    draw_needed = true;
                }
                TermEvent::FocusGained | TermEvent::FocusLost => {}
            }
        }

        let watch = watcher.poll();
        if watch.changed {
            reload_due = Some(Instant::now() + RELOAD_DEBOUNCE);
        }
        if !watch.errors.is_empty() {
            state.last_status = Some(format!("watch error: {}", watch.errors.join("; ")));
            draw_needed = true;
        }

        if reload_due.is_some_and(|due| Instant::now() >= due) {
            let (cols, rows) = terminal.viewport_size()?;
            viewport_cols = cols;
            viewport_rows = rows;
            state.reload(cols);
            state.scroll = clamp_offset(
                state.scroll,
                total_rows(&state.lines),
                viewport_rows as usize,
            );
            reload_due = None;
            draw_needed = true;
        }

        if draw_needed {
            let reload_state = reload_due.map(|_| "pending reload");
            terminal.draw(
                &state.lines,
                state.scroll,
                &state.status(viewport_rows as usize, reload_state),
                state.selection_range(),
                state.search_highlights(),
            )?;
            draw_needed = false;
        }
    }

    Ok(())
}

#[derive(Debug)]
struct ViewerState {
    path: PathBuf,
    source: String,
    lines: Vec<RenderLine>,
    scroll: usize,
    last_status: Option<String>,
    selection: SelectionState,
    search: SearchState,
}

#[derive(Debug, Clone, Default)]
struct SearchState {
    input: Option<String>,
    query: String,
    matches: Vec<SelectionRange>,
    current: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchKeyResult {
    Redraw,
    None,
}

impl ViewerState {
    fn load(path: PathBuf, width: u16) -> Result<Self, AppError> {
        let source = load_source(&path)?;
        let lines = render_markdown(&path, &source, width);
        Ok(Self {
            path,
            source,
            lines,
            scroll: 0,
            last_status: Some("loaded".to_string()),
            selection: SelectionState::default(),
            search: SearchState::default(),
        })
    }

    fn reload(&mut self, width: u16) {
        match load_source(&self.path) {
            Ok(source) => {
                self.source = source;
                self.render(width);
                self.last_status = Some("reloaded".to_string());
            }
            Err(err) => {
                self.last_status = Some(format!("reload error: {err}"));
                if self.lines.is_empty() {
                    self.lines = vec![plain_line(format!(
                        "failed to read {}: {err}",
                        self.path.display()
                    ))];
                }
            }
        }
    }

    fn render(&mut self, width: u16) {
        self.lines = render_markdown(&self.path, &self.source, width);
        if self.lines.is_empty() {
            self.lines = vec![plain_line("(empty)")];
        }
        self.selection.clear();
        self.refresh_search_matches();
    }

    fn scroll_by(&mut self, delta: isize, viewport_rows: usize) {
        self.scroll = scroll_by(self.scroll, delta, total_rows(&self.lines), viewport_rows);
    }

    fn max_scroll(&self, viewport_rows: usize) -> usize {
        crate::scroll::max_offset(total_rows(&self.lines), viewport_rows)
    }

    fn status(&self, viewport_rows: usize, reload_state: Option<&str>) -> String {
        if let Some(input) = &self.search.input {
            return format!("/{input}");
        }

        let total = total_rows(&self.lines);
        let percent = if total <= viewport_rows {
            100
        } else {
            ((self.scroll * 100) / self.max_scroll(viewport_rows).max(1)).min(100)
        };
        let state = reload_state
            .or(self.last_status.as_deref())
            .unwrap_or("watching");
        format!(
            "{} | {}% | row {}/{} | {}",
            self.path.display(),
            percent,
            self.scroll.saturating_add(1),
            total.max(1),
            state
        )
    }

    fn selection_range(&self) -> Option<crate::selection::SelectionRange> {
        self.selection.range()
    }

    fn search_highlights(&self) -> &[SelectionRange] {
        &self.search.matches
    }

    fn selection_text(&self, max_width: usize) -> Option<String> {
        selected_text(&self.lines, self.selection_range()?, max_width)
    }

    fn selection_status(&self, max_width: usize) -> Option<String> {
        let text = self.selection_text(max_width)?;
        Some(format!(
            "selected {} chars; press y/c/Enter to copy",
            text.chars().count()
        ))
    }

    fn copy_selection(&mut self, terminal: &mut Terminal, max_width: usize) -> io::Result<()> {
        let Some(text) = self.selection_text(max_width) else {
            self.last_status = Some("no selection".to_string());
            return Ok(());
        };
        terminal.copy_to_clipboard(&text)?;
        self.last_status = Some(format!("copied {} chars", text.chars().count()));
        Ok(())
    }

    fn mouse_point(&self, mouse: MouseEvent, viewport_rows: usize) -> Option<SelectionPoint> {
        if mouse.row as usize >= viewport_rows {
            return None;
        }
        Some(SelectionPoint::new(
            self.scroll.saturating_add(mouse.row as usize),
            mouse.column as usize,
        ))
    }

    fn search_input_active(&self) -> bool {
        self.search.input.is_some()
    }

    fn begin_search_input(&mut self) {
        self.search.input = Some(String::new());
    }

    fn handle_search_input(&mut self, key: KeyEvent, viewport_rows: usize) -> SearchKeyResult {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.search.input = None;
                self.last_status = Some("search canceled".to_string());
                SearchKeyResult::Redraw
            }
            (KeyCode::Enter, _) => {
                let input = self.search.input.take().unwrap_or_default();
                if input.is_empty() {
                    self.clear_search();
                } else {
                    self.commit_search(input, viewport_rows);
                }
                SearchKeyResult::Redraw
            }
            (KeyCode::Backspace, _) => {
                if let Some(input) = &mut self.search.input {
                    input.pop();
                }
                SearchKeyResult::Redraw
            }
            (KeyCode::Char(ch), KeyModifiers::NONE) | (KeyCode::Char(ch), KeyModifiers::SHIFT) => {
                if let Some(input) = &mut self.search.input {
                    input.push(ch);
                }
                SearchKeyResult::Redraw
            }
            _ => SearchKeyResult::None,
        }
    }

    fn clear_search(&mut self) {
        self.search.query.clear();
        self.search.matches.clear();
        self.search.current = None;
        self.search.input = None;
        self.last_status = Some("search cleared".to_string());
    }

    fn commit_search(&mut self, query: String, viewport_rows: usize) {
        self.search.query = query;
        self.search.matches = find_matches(&self.lines, &self.search.query);
        self.search.current = first_at_or_after(&self.search.matches, self.scroll);
        self.scroll_to_current_search_match(viewport_rows);
        self.last_status = Some(self.search_status());
    }

    fn refresh_search_matches(&mut self) {
        if self.search.query.is_empty() {
            self.search.matches.clear();
            self.search.current = None;
            return;
        }

        let row = self
            .search
            .current
            .and_then(|index| self.search.matches.get(index))
            .map(|range| range.start.row)
            .unwrap_or(self.scroll);
        self.search.matches = find_matches(&self.lines, &self.search.query);
        self.search.current = first_at_or_after(&self.search.matches, row);
    }

    fn next_search_match(&mut self, viewport_rows: usize) {
        if self.search.query.is_empty() {
            self.last_status = Some("no active search".to_string());
            return;
        }
        self.search.current = next_index(&self.search.matches, self.search.current);
        self.scroll_to_current_search_match(viewport_rows);
        self.last_status = Some(self.search_status());
    }

    fn previous_search_match(&mut self, viewport_rows: usize) {
        if self.search.query.is_empty() {
            self.last_status = Some("no active search".to_string());
            return;
        }
        self.search.current = previous_index(&self.search.matches, self.search.current);
        self.scroll_to_current_search_match(viewport_rows);
        self.last_status = Some(self.search_status());
    }

    fn scroll_to_current_search_match(&mut self, viewport_rows: usize) {
        if let Some(range) = self
            .search
            .current
            .and_then(|index| self.search.matches.get(index))
        {
            self.scroll = clamp_offset(range.start.row, total_rows(&self.lines), viewport_rows);
        }
    }

    fn search_status(&self) -> String {
        let total = self.search.matches.len();
        if total == 0 {
            return format!("no matches: {}", self.search.query);
        }
        let current = self.search.current.unwrap_or(0).saturating_add(1);
        format!("match {current}/{total}: {}", self.search.query)
    }
}

fn load_source(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    Quit,
    Scroll(isize),
    PageUp,
    PageDown,
    Top,
    Bottom,
    Copy,
    Search,
    NextSearch,
    PreviousSearch,
    None,
}

pub fn input_action(key: KeyEvent) -> InputAction {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => InputAction::Copy,
        (KeyCode::Char('q'), KeyModifiers::NONE)
        | (KeyCode::Esc, _)
        | (KeyCode::Char('Q'), KeyModifiers::SHIFT) => InputAction::Quit,
        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => InputAction::Scroll(1),
        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => InputAction::Scroll(-1),
        (KeyCode::PageDown, _) => InputAction::PageDown,
        (KeyCode::PageUp, _) => InputAction::PageUp,
        (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => InputAction::Top,
        (KeyCode::End, _) | (KeyCode::Char('G'), KeyModifiers::SHIFT) => InputAction::Bottom,
        (KeyCode::Char('/'), KeyModifiers::NONE) => InputAction::Search,
        (KeyCode::Char('n'), KeyModifiers::NONE) | (KeyCode::Char('N'), KeyModifiers::SHIFT) => {
            InputAction::NextSearch
        }
        (KeyCode::Char('p'), KeyModifiers::NONE) | (KeyCode::Char('P'), KeyModifiers::SHIFT) => {
            InputAction::PreviousSearch
        }
        (KeyCode::Char('y'), KeyModifiers::NONE)
        | (KeyCode::Char('c'), KeyModifiers::NONE)
        | (KeyCode::Enter, _) => InputAction::Copy,
        _ => InputAction::None,
    }
}

fn is_control_c(key: KeyEvent) -> bool {
    matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn maps_keyboard_controls() {
        assert_eq!(
            input_action(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            InputAction::Scroll(1)
        );
        assert_eq!(
            input_action(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)),
            InputAction::Scroll(-1)
        );
        assert_eq!(
            input_action(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            InputAction::PageDown
        );
        assert_eq!(
            input_action(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            InputAction::Copy
        );
        assert_eq!(
            input_action(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)),
            InputAction::Copy
        );
        assert_eq!(
            input_action(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)),
            InputAction::Copy
        );
        assert_eq!(
            input_action(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
            InputAction::Search
        );
        assert_eq!(
            input_action(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
            InputAction::NextSearch
        );
        assert_eq!(
            input_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
            InputAction::PreviousSearch
        );
    }

    #[test]
    fn viewer_state_loads_and_reloads() {
        let dir = std::env::temp_dir().join(format!(
            "mdview-state-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.md");
        fs::write(&path, "# Old").unwrap();
        let mut state = ViewerState::load(path.clone(), 80).unwrap();
        assert!(crate::markdown::flatten_text(&state.lines)[0].contains("Old"));

        fs::write(&path, "# New").unwrap();
        state.reload(80);
        assert!(crate::markdown::flatten_text(&state.lines)[0].contains("New"));

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(dir);
    }

    #[test]
    fn search_starts_from_viewport_top() {
        let mut state = state_with_lines(&[
            "needle before viewport",
            "filler",
            "Needle in viewport",
            "needle after viewport",
        ]);
        state.scroll = 2;

        state.commit_search("needle".to_string(), 2);

        assert_eq!(state.search.matches.len(), 3);
        assert_eq!(state.search.current, Some(1));
        assert_eq!(state.scroll, 2);
        assert!(state.last_status.as_deref().unwrap().contains("match 2/3"));
    }

    #[test]
    fn search_next_and_previous_navigate_matches() {
        let mut state = state_with_lines(&["needle one", "needle two", "needle three"]);
        state.commit_search("needle".to_string(), 2);

        assert_eq!(state.search.current, Some(0));
        state.next_search_match(2);
        assert_eq!(state.search.current, Some(1));
        assert_eq!(state.scroll, 1);
        state.next_search_match(2);
        assert_eq!(state.search.current, Some(2));
        state.previous_search_match(2);
        assert_eq!(state.search.current, Some(1));
    }

    #[test]
    fn empty_search_clears_highlights_and_preserves_scroll() {
        let mut state = state_with_lines(&["needle one", "filler", "needle two", "tail"]);
        state.commit_search("needle".to_string(), 2);
        state.scroll = 2;
        state.begin_search_input();

        assert_eq!(
            state.handle_search_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 2),
            SearchKeyResult::Redraw
        );

        assert_eq!(state.scroll, 2);
        assert!(state.search.query.is_empty());
        assert!(state.search.matches.is_empty());
        assert_eq!(state.last_status.as_deref(), Some("search cleared"));
    }

    #[test]
    fn search_input_status_shows_prompt() {
        let mut state = state_with_lines(&["alpha"]);
        state.begin_search_input();
        let _ =
            state.handle_search_input(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE), 10);

        assert_eq!(state.status(10, None), "/a");
    }

    #[test]
    fn control_c_is_always_reserved_for_copying() {
        assert!(is_control_c(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_control_c(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::NONE
        )));
    }

    fn state_with_lines(lines: &[&str]) -> ViewerState {
        ViewerState {
            path: PathBuf::from("/tmp/doc.md"),
            source: lines.join("\n"),
            lines: lines.iter().map(|line| plain_line(*line)).collect(),
            scroll: 0,
            last_status: Some("loaded".to_string()),
            selection: SelectionState::default(),
            search: SearchState::default(),
        }
    }
}
