mod app;
mod git;
mod graphics;
mod herdr;
mod model;
mod raster;
mod search;
mod state;
mod ui;
mod view;

use std::{
    collections::HashSet,
    env,
    io::{self, Write},
    process::ExitCode,
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::{Action, App};
use view::{Target, View};

const PLUGIN_ID: &str = "dartyuhov.blink";
const PICKER_ENTRYPOINT: &str = "picker";
const TICK: Duration = Duration::from_millis(80);

fn main() -> ExitCode {
    let result = match env::args().nth(1).as_deref() {
        Some("open") => open(),
        Some("ui") => run_ui(),
        Some("event") => state::run_event_hook().map_err(|e| e.to_string()),
        Some("list") => list(),
        _ => Err(
            "usage: herdr-blink <open|ui|event|list [--view agents|workspaces|projects] [query]>"
                .into(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("herdr-blink: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `open` action: launch the popup. A popup is session-modal and receives
/// all input, so if one is already open herdr reports it busy and the
/// existing popup simply stays focused.
fn open() -> Result<(), String> {
    let plugin = env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| PLUGIN_ID.into());
    match herdr::open_plugin_pane(&plugin, PICKER_ENTRYPOINT) {
        Ok(()) => Ok(()),
        Err(herdr::Error::Api { code, .. }) if code == "ui_busy" => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn load_items() -> Result<(Vec<model::Item>, serde_json::Value), String> {
    let snapshot = herdr::snapshot().map_err(|e| e.to_string())?;
    let state = state::load(&state::state_dir());
    let items = model::build_items(&snapshot, &state, &mut git::GitCache::default());
    Ok((items, snapshot))
}

/// `list [--view agents|workspaces|projects] [query]`: prints the rows of a
/// view, trees indented by depth.
fn list() -> Result<(), String> {
    let mut view = View::Agents;
    let mut query = None;
    let mut args = env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--view" => {
                let name = args.next().unwrap_or_default();
                view = View::parse(&name).ok_or_else(|| format!("unknown view: {name:?}"))?;
            }
            _ => query = Some(arg),
        }
    }
    let (items, _) = load_items()?;
    let mut app = App::new(items);
    app.set_view(view);
    if let Some(query) = query {
        app.set_query(&query);
    }
    for row in &app.rows {
        let indent = "  ".repeat(row.depth as usize);
        if let Some(summary) = &row.summary {
            let detail = summary
                .detail
                .as_ref()
                .map(|d| format!("  {d}"))
                .unwrap_or_default();
            let counts: Vec<String> = view::BADGES
                .iter()
                .zip(&summary.counts)
                .filter(|(_, n)| **n > 0)
                .map(|(s, n)| format!("{} {n}", s.word()))
                .collect();
            println!("{indent}{}{detail}\t{}", summary.label, counts.join(" "));
            continue;
        }
        let Some(i) = row.item() else { continue };
        let i = &app.items[i];
        println!(
            "{indent}{}\t{}\t{}\t{}\t{} › {}\t{}{}",
            i.pane_id,
            i.agent.as_deref().unwrap_or("$"),
            if i.is_agent() { i.status.word() } else { "" },
            i.title,
            i.workspace,
            i.tab,
            i.folder_hint(),
            if i.focused { "\there" } else { "" }
        );
    }
    Ok(())
}

fn run_ui() -> Result<(), String> {
    let (items, snapshot) = load_items()?;
    let mut app = App::new(items);

    let mut stdout = io::stdout();
    // Popup-local background so the popup renders opaque, like the user's
    // other `#1e2030` popups.
    let _ = write!(stdout, "\x1b]11;#1e2030\x07");
    terminal::enable_raw_mode().map_err(|e| e.to_string())?;
    execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
    // Unambiguous Esc (no Esc+key → Alt+key merging); ignored by terminals
    // without the kitty keyboard protocol.
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout)).map_err(|e| e.to_string())?;

    let mut graphics = graphics::Graphics::init();
    let result = ui_loop(&mut terminal, &mut app, &snapshot, &mut graphics);

    if let Some(g) = graphics.as_mut() {
        g.clear(terminal.backend_mut());
    }
    let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    let _ = terminal::disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    let Some(target) = result? else {
        return Ok(());
    };
    let pane_id = match target {
        Target::Agent(pane_id) => {
            herdr::focus_agent(&pane_id).map_err(|e| e.to_string())?;
            pane_id
        }
        Target::Pane(pane_id) => {
            herdr::focus_pane(&pane_id).map_err(|e| e.to_string())?;
            pane_id
        }
        Target::Tab(tab_id) => return herdr::focus_tab(&tab_id).map_err(|e| e.to_string()),
        Target::Workspace(workspace_id) => {
            return herdr::focus_workspace(&workspace_id).map_err(|e| e.to_string());
        }
        Target::Close => return Ok(()),
    };
    let now = state::now_ms();
    let _ = state::update(&state::state_dir(), |s| {
        state::apply_event(s, state::EventKind::Focused, pane_id, now)
    });
    Ok(())
}

type Term = Terminal<CrosstermBackend<io::Stdout>>;

fn ui_loop(
    terminal: &mut Term,
    app: &mut App,
    snapshot: &serde_json::Value,
    graphics: &mut Option<graphics::Graphics>,
) -> Result<Option<Target>, String> {
    let mut ui = ui::UiState::default();
    if let Some(g) = graphics.as_ref() {
        ui.image_logos = g.available().clone();
    }
    terminal
        .draw(|f| ui::render(f, app, &mut ui))
        .map_err(|e| e.to_string())?;
    // Housekeeping waits until the first frame is on screen.
    prune_state(snapshot);

    let mut last_slots = Vec::new();
    let mut last_tick = Instant::now();
    let mut dirty = true;

    loop {
        if dirty {
            terminal
                .draw(|f| ui::render(f, app, &mut ui))
                .map_err(|e| e.to_string())?;
            if let Some(g) = graphics.as_mut()
                && ui.logo_slots != last_slots
            {
                g.draw(terminal.backend_mut(), &ui.logo_slots);
                last_slots = ui.logo_slots.clone();
            }
            dirty = false;
        }

        let timeout = TICK.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                Event::Key(key) if key.kind != KeyEventKind::Release => match app.handle_key(key) {
                    Action::Quit => return Ok(None),
                    Action::Jump(target) => return Ok(Some(target)),
                    Action::None => dirty = true,
                },
                Event::Resize(..) => {
                    // Placement depends on geometry; force a re-upload. The
                    // cell size may only become known after herdr's first
                    // resize, so retry graphics if they were unavailable.
                    if graphics.is_none() {
                        *graphics = graphics::Graphics::init();
                        if let Some(g) = graphics.as_ref() {
                            ui.image_logos = g.available().clone();
                        }
                    }
                    last_slots.clear();
                    dirty = true;
                }
                _ => {}
            }
        }
        if last_tick.elapsed() >= TICK {
            last_tick = Instant::now();
            ui.tick = ui.tick.wrapping_add(1);
            // Only the working spinner animates.
            if app.animating() {
                dirty = true;
            }
        }
    }
}

/// Lazily drops state entries for panes that no longer exist.
fn prune_state(snapshot: &serde_json::Value) {
    let Some(panes) = snapshot.get("panes").and_then(|p| p.as_array()) else {
        return;
    };
    let live: HashSet<&str> = panes
        .iter()
        .filter_map(|p| p.get("pane_id")?.as_str())
        .collect();
    if live.is_empty() {
        return;
    }
    let dir = state::state_dir();
    let current = state::load(&dir);
    if current.keys().all(|id| live.contains(id.as_str())) {
        return;
    }
    let _ = state::update(&dir, |s| state::prune(s, &live));
}
