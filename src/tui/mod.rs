//! Interactive terminal UI built on ratatui.
//!
//! Flow: fuzzy-find a key -> select services -> select file kinds ->
//! choose set/generate -> preview diff -> confirm & apply.
//!
//! How a TUI works, briefly: we take over the terminal, then loop forever doing
//! two things — (1) *draw* the current screen, and (2) *read* one key press and
//! update our state. `ratatui` handles drawing; `crossterm` handles input. All
//! the state lives in the `App` struct, and `Screen` tracks which step we're on.

use anyhow::{Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::cli::{Cli, context, wizard};
use crate::config::Config;
use crate::edit::{self, ChangeSet};
use crate::model::{FileKind, Service};
use crate::search;
use crate::secret::{self, SecretSpec};

/// The file kinds shown on the "Files" screen, in display order.
const KINDS: [FileKind; 4] = [
    FileKind::Dotenv,
    FileKind::DotenvExample,
    FileKind::ConfigMap,
    FileKind::Secret,
];

/// Launch the interactive TUI.
pub fn launch(cli: &Cli) -> Result<()> {
    let cwd = std::env::current_dir()?;
    // On first ever run (no nv.yml), offer to create one before starting.
    if !cli.no_config {
        wizard::first_run_if_needed(&cwd)?;
    }

    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    if ctx.services.is_empty() {
        bail!("no services found; run `nv init` or use --root <dir>");
    }

    let app = App::new(ctx.services.clone(), ctx.config.clone(), cli.dry_run);
    // `ratatui::init()` switches the terminal into full-screen "raw" mode.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, app);
    // Always restore the normal terminal afterwards, even if `run` errored.
    ratatui::restore();
    result
}

/// Which screen the app is currently showing. The UI is a small state machine
/// that advances through these in order (Find -> ... -> Preview).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Find,     // type a query and pick a key
    Services, // tick which services to change
    Files,    // tick which file kinds to change
    Action,   // choose "set a value" or "generate a secret"
    Value,    // type the value (only for "set")
    Preview,  // review the diff and confirm
}

/// The two things a user can do to a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Set,
    Generate,
}

/// All mutable state for the running UI. Fields grouped by screen: the "list"
/// fields (`*_state`) remember which row is highlighted; the `*_sel` booleans
/// remember which items are ticked in the multi-select screens.
struct App {
    services: Vec<Service>,
    config: Option<Config>,
    dry_run: bool,

    screen: Screen,
    status: String,

    all_keys: Vec<String>,
    query: String,
    matches: Vec<String>,
    key_state: ListState,
    selected_key: Option<String>,

    service_names: Vec<String>,
    service_sel: Vec<bool>,
    service_state: ListState,

    kind_sel: [bool; 4],
    kind_state: ListState,

    action: Action,
    action_state: ListState,

    value_input: String,

    changeset: Option<ChangeSet>,
    preview_scroll: u16,
}

impl App {
    fn new(services: Vec<Service>, config: Option<Config>, dry_run: bool) -> Self {
        let index = search::build_index(&services);
        let all_keys = search::distinct_keys(&index);
        let matches = all_keys.clone();
        let service_names: Vec<String> = services.iter().map(|s| s.name.clone()).collect();
        let service_sel = vec![true; service_names.len()];

        let mut key_state = ListState::default();
        if !matches.is_empty() {
            key_state.select(Some(0));
        }
        let mut service_state = ListState::default();
        service_state.select(Some(0));
        let mut kind_state = ListState::default();
        kind_state.select(Some(0));
        let mut action_state = ListState::default();
        action_state.select(Some(0));

        App {
            services,
            config,
            dry_run,
            screen: Screen::Find,
            status: "Type to search, Up/Down to move, Enter to pick, Esc to quit".to_string(),
            all_keys,
            query: String::new(),
            matches,
            key_state,
            selected_key: None,
            service_names,
            service_sel,
            service_state,
            kind_sel: [true, true, true, true],
            kind_state,
            action: Action::Set,
            action_state,
            value_input: String::new(),
            changeset: None,
            preview_scroll: 0,
        }
    }

    fn refresh_matches(&mut self) {
        self.matches = search::fuzzy_strings(&self.all_keys, &self.query);
        if self.matches.is_empty() {
            self.key_state.select(None);
        } else {
            let sel = self
                .key_state
                .selected()
                .unwrap_or(0)
                .min(self.matches.len() - 1);
            self.key_state.select(Some(sel));
        }
    }

    fn move_selection(state: &mut ListState, len: usize, delta: isize) {
        if len == 0 {
            return;
        }
        let current = state.selected().unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(len as isize);
        state.select(Some(next as usize));
    }

    fn selected_kinds(&self) -> Vec<FileKind> {
        KINDS
            .iter()
            .enumerate()
            .filter(|(i, _)| self.kind_sel[*i])
            .map(|(_, k)| *k)
            .collect()
    }

    fn selected_service_names(&self) -> Vec<String> {
        self.service_names
            .iter()
            .enumerate()
            .filter(|(i, _)| self.service_sel[*i])
            .map(|(_, n)| n.clone())
            .collect()
    }

    /// Build the change set from current selections and the chosen action.
    ///
    /// Mirrors the CLI `set`/`gen` logic: turn the ticked services + file kinds
    /// into targets, then compute the value for each. Generated secrets are left
    /// empty in example files, just like on the command line.
    fn build_changeset(&mut self) -> Result<()> {
        let key = match &self.selected_key {
            Some(k) => k.clone(),
            None => bail!("no key selected"),
        };
        let service_filter = self.selected_service_names();
        let kind_filter = self.selected_kinds();
        let targets = edit::collect_targets(&self.services, &service_filter, &kind_filter);
        if targets.is_empty() {
            bail!("no matching files for the current selection");
        }

        let changeset = match self.action {
            Action::Set => {
                let value = self.value_input.clone();
                ChangeSet::build(&targets, &key, |_| value.clone())?
            }
            Action::Generate => {
                let spec = self
                    .config
                    .as_ref()
                    .and_then(|c| c.secret_preset(&key))
                    .map(SecretSpec::from)
                    .unwrap_or_default();
                let shared = secret::generate(&spec)?;
                ChangeSet::build(&targets, &key, |t| {
                    if t.file.kind.is_example() {
                        String::new()
                    } else {
                        shared.clone()
                    }
                })?
            }
        };
        self.changeset = Some(changeset);
        Ok(())
    }
}

/// Main event loop: draw, wait for a key, react, repeat until the user quits.
fn run(terminal: &mut ratatui::DefaultTerminal, mut app: App) -> Result<()> {
    loop {
        // 1. Redraw the whole screen based on current state.
        terminal.draw(|f| draw(f, &mut app))?;

        // 2. Block until the next input event; ignore anything that isn't a
        //    key *press* (e.g. key releases, mouse events).
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        // Esc always steps back one screen / quits from the first screen.
        if key.code == KeyCode::Esc {
            match app.screen {
                Screen::Find => return Ok(()),
                Screen::Services => app.screen = Screen::Find,
                Screen::Files => app.screen = Screen::Services,
                Screen::Action => app.screen = Screen::Files,
                Screen::Value => app.screen = Screen::Action,
                Screen::Preview => app.screen = Screen::Action,
            }
            continue;
        }

        // 3. Otherwise, hand the key to the handler for the current screen.
        match app.screen {
            Screen::Find => handle_find(&mut app, key.code),
            Screen::Services => handle_services(&mut app, key.code),
            Screen::Files => handle_files(&mut app, key.code),
            Screen::Action => handle_action(&mut app, key.code),
            Screen::Value => handle_value(&mut app, key.code),
            Screen::Preview => {
                if let Some(done) = handle_preview(&mut app, key.code)?
                    && done
                {
                    return Ok(());
                }
            }
        }
    }
}

fn handle_find(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char(c) => {
            app.query.push(c);
            app.refresh_matches();
        }
        KeyCode::Backspace => {
            app.query.pop();
            app.refresh_matches();
        }
        KeyCode::Up => App::move_selection(&mut app.key_state, app.matches.len(), -1),
        KeyCode::Down => App::move_selection(&mut app.key_state, app.matches.len(), 1),
        KeyCode::Enter => {
            if let Some(i) = app.key_state.selected()
                && let Some(k) = app.matches.get(i)
            {
                app.selected_key = Some(k.clone());
                app.screen = Screen::Services;
                app.status = "Space to toggle services, 'a' all, Enter to continue".to_string();
            }
        }
        _ => {}
    }
}

fn handle_services(app: &mut App, code: KeyCode) {
    let len = app.service_names.len();
    match code {
        KeyCode::Up => App::move_selection(&mut app.service_state, len, -1),
        KeyCode::Down => App::move_selection(&mut app.service_state, len, 1),
        KeyCode::Char(' ') => {
            if let Some(i) = app.service_state.selected() {
                app.service_sel[i] = !app.service_sel[i];
            }
        }
        KeyCode::Char('a') => {
            let all = app.service_sel.iter().all(|&b| b);
            app.service_sel.iter_mut().for_each(|b| *b = !all);
        }
        KeyCode::Enter => {
            if app.service_sel.iter().any(|&b| b) {
                app.screen = Screen::Files;
                app.status = "Space to toggle file kinds, Enter to continue".to_string();
            } else {
                app.status = "Select at least one service".to_string();
            }
        }
        _ => {}
    }
}

fn handle_files(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => App::move_selection(&mut app.kind_state, KINDS.len(), -1),
        KeyCode::Down => App::move_selection(&mut app.kind_state, KINDS.len(), 1),
        KeyCode::Char(' ') => {
            if let Some(i) = app.kind_state.selected() {
                app.kind_sel[i] = !app.kind_sel[i];
            }
        }
        KeyCode::Char('a') => {
            let all = app.kind_sel.iter().all(|&b| b);
            app.kind_sel.iter_mut().for_each(|b| *b = !all);
        }
        KeyCode::Enter => {
            if app.kind_sel.iter().any(|&b| b) {
                app.screen = Screen::Action;
                app.status = "Choose an action".to_string();
            } else {
                app.status = "Select at least one file kind".to_string();
            }
        }
        _ => {}
    }
}

fn handle_action(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up => App::move_selection(&mut app.action_state, 2, -1),
        KeyCode::Down => App::move_selection(&mut app.action_state, 2, 1),
        KeyCode::Enter => {
            app.action = if app.action_state.selected() == Some(0) {
                Action::Set
            } else {
                Action::Generate
            };
            match app.action {
                Action::Set => {
                    app.value_input.clear();
                    app.screen = Screen::Value;
                    app.status = "Type a value, Enter to preview".to_string();
                }
                Action::Generate => {
                    if let Err(e) = app.build_changeset() {
                        app.status = format!("Error: {e}");
                    } else {
                        app.preview_scroll = 0;
                        app.screen = Screen::Preview;
                        app.status = preview_status(app);
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_value(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char(c) => app.value_input.push(c),
        KeyCode::Backspace => {
            app.value_input.pop();
        }
        KeyCode::Enter => {
            if let Err(e) = app.build_changeset() {
                app.status = format!("Error: {e}");
            } else {
                app.preview_scroll = 0;
                app.screen = Screen::Preview;
                app.status = preview_status(app);
            }
        }
        _ => {}
    }
}

fn handle_preview(app: &mut App, code: KeyCode) -> Result<Option<bool>> {
    match code {
        KeyCode::Up => app.preview_scroll = app.preview_scroll.saturating_sub(1),
        KeyCode::Down => app.preview_scroll = app.preview_scroll.saturating_add(1),
        KeyCode::Char('y') | KeyCode::Enter => {
            if app.dry_run {
                app.status = "Dry run: no files written. Esc to go back.".to_string();
                return Ok(None);
            }
            if let Some(cs) = &app.changeset {
                let written = cs.apply()?;
                app.status = format!("Wrote {written} file(s). Esc to quit.");
            }
        }
        KeyCode::Char('n') => {
            app.screen = Screen::Action;
        }
        _ => {}
    }
    Ok(None)
}

fn preview_status(app: &App) -> String {
    if app.dry_run {
        "DRY RUN — review diff, Esc to go back".to_string()
    } else {
        "y/Enter to apply, n to cancel, Up/Down to scroll".to_string()
    }
}

// ----------------------------------------------------------------------------
// Rendering
// ----------------------------------------------------------------------------

/// Draw the whole screen: a one-line title bar, the main body for the current
/// screen, and a one-line status bar at the bottom.
fn draw(f: &mut Frame, app: &mut App) {
    // Split the terminal vertically into three rows: title, body, status.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title (1 line tall)
            Constraint::Min(3),    // body (takes the remaining space)
            Constraint::Length(1), // status (1 line tall)
        ])
        .split(f.area());

    let title = Line::from(vec![
        Span::styled(
            "nv",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(breadcrumb(app)),
    ]);
    f.render_widget(Paragraph::new(title), chunks[0]);

    // The body depends on which screen we're on.
    match app.screen {
        Screen::Find => draw_find(f, app, chunks[1]),
        Screen::Services => draw_services(f, app, chunks[1]),
        Screen::Files => draw_files(f, app, chunks[1]),
        Screen::Action => draw_action(f, app, chunks[1]),
        Screen::Value => draw_value(f, app, chunks[1]),
        Screen::Preview => draw_preview(f, app, chunks[1]),
    }

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            app.status.clone(),
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[2],
    );
}

fn breadcrumb(app: &App) -> String {
    let key = app.selected_key.as_deref().unwrap_or("-");
    match app.screen {
        Screen::Find => "find key".to_string(),
        Screen::Services => format!("{key} > services"),
        Screen::Files => format!("{key} > files"),
        Screen::Action => format!("{key} > action"),
        Screen::Value => format!("{key} > value"),
        Screen::Preview => format!("{key} > preview"),
    }
}

fn draw_find(f: &mut Frame, app: &mut App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    let input = Paragraph::new(app.query.clone())
        .block(Block::default().borders(Borders::ALL).title("Search key"));
    f.render_widget(input, rows[0]);

    let items: Vec<ListItem> = app
        .matches
        .iter()
        .map(|k| ListItem::new(k.clone()))
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Keys"))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, rows[1], &mut app.key_state);
}

fn draw_services(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .service_names
        .iter()
        .enumerate()
        .map(|(i, name)| ListItem::new(format!("[{}] {name}", check(app.service_sel[i]))))
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Services"))
        .highlight_style(Style::default().bg(Color::Blue))
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut app.service_state);
}

fn draw_files(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = KINDS
        .iter()
        .enumerate()
        .map(|(i, k)| ListItem::new(format!("[{}] {}", check(app.kind_sel[i]), k.label())))
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("File kinds"))
        .highlight_style(Style::default().bg(Color::Blue))
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut app.kind_state);
}

fn draw_action(f: &mut Frame, app: &mut App, area: Rect) {
    let items = vec![
        ListItem::new("Set a value"),
        ListItem::new("Generate a secret"),
    ];
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Action"))
        .highlight_style(Style::default().bg(Color::Blue))
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut app.action_state);
}

fn draw_value(f: &mut Frame, app: &mut App, area: Rect) {
    let key = app.selected_key.as_deref().unwrap_or("");
    let input = Paragraph::new(app.value_input.clone()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Value for {key}")),
    );
    f.render_widget(input, area);
}

fn draw_preview(f: &mut Frame, app: &mut App, area: Rect) {
    // Pass `use_color: false` since the TUI applies its own ratatui styles.
    let colors = crate::color::ColorConfig::default();
    let diff = app
        .changeset
        .as_ref()
        .map(|c| c.render_diff(&colors, false))
        .unwrap_or_default();

    let lines: Vec<Line> = diff
        .lines()
        .map(|l| {
            let style = if l.contains(" + ") {
                // Diff addition line (tree characters before + sign).
                Style::default().fg(Color::Green)
            } else if l.contains(" - ") {
                // Diff removal line (tree characters before - sign).
                Style::default().fg(Color::Red)
            } else if l.ends_with('/') {
                // Service name — highlight like the old header.
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::from(Span::styled(l.to_string(), style))
        })
        .collect();

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Preview"))
        .wrap(Wrap { trim: false })
        .scroll((app.preview_scroll, 0));
    f.render_widget(para, area);
}

fn check(selected: bool) -> char {
    if selected { 'x' } else { ' ' }
}
