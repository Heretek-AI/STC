//! Studio TUI: terminal-first cockpit surface (Phase 4).
//! Four views over a `studio.db` read projection (100ms CDC poll):
//! War Room (fleet board) | Agent Stream (terminal logs) |
//! Audit & Gatekeeper (evidence-first diffs) | MCP Registry (matrix + burn).
//! The UI never owns state: every frame re-reads the DB and stamps staleness.

use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use studio_core::state::StateStore;

const POLL_MS: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    WarRoom,
    Stream,
    Audit,
    Registry,
}

impl View {
    fn next(self) -> Self {
        match self {
            View::WarRoom => View::Stream,
            View::Stream => View::Audit,
            View::Audit => View::Registry,
            View::Registry => View::WarRoom,
        }
    }
    fn title(self) -> &'static str {
        match self {
            View::WarRoom => "1 War Room",
            View::Stream => "2 Agent Stream",
            View::Audit => "3 Audit & Gatekeeper",
            View::Registry => "4 MCP Registry",
        }
    }
}

fn lane_color(lane: &str) -> Color {
    match lane {
        "building" => Color::Blue,
        "validating" => Color::Yellow,
        "in-review" => Color::Magenta,
        _ => Color::Green,
    }
}

fn render(f: &mut Frame, view: View, snap: &studio_core::projection::Snapshot) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());
    // header: all four views + staleness badge (GUI wrong if it disagrees with db)
    let runtime_label = if snap.runtime_mode == "privileged-dev" {
        " · DEV-MODE (root-equiv)".to_string()
    } else {
        format!(" · {}", snap.runtime_mode)
    };
    let runtime_color = if snap.runtime_mode == "privileged-dev" {
        Color::Red
    } else {
        Color::DarkGray
    };
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "STUDIO ",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::White),
        ),
        Span::raw(
            [View::WarRoom, View::Stream, View::Audit, View::Registry]
                .iter()
                .map(|v| {
                    if *v == view {
                        format!("[{}] ", v.title())
                    } else {
                        format!("{} ", v.title())
                    }
                })
                .collect::<String>(),
        ),
        Span::styled(snap.stale_badge(), Style::default().fg(Color::DarkGray)),
        Span::styled(runtime_label, Style::default().fg(runtime_color)),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    match view {
        View::WarRoom => {
            let items: Vec<ListItem> = snap
                .fleet
                .iter()
                .map(|t| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{} ", t.lane),
                            Style::default().fg(lane_color(&t.lane)),
                        ),
                        Span::raw(format!("{} {} {}", t.id, t.kind, t.status)),
                    ]))
                })
                .collect();
            f.render_widget(
                List::new(items).block(
                    Block::default()
                        .title("Fleet board: building→validating→in-review→ready")
                        .borders(Borders::ALL),
                ),
                chunks[1],
            );
        }
        View::Stream => {
            let items: Vec<ListItem> = snap
                .stream
                .iter()
                .take(50)
                .map(|e| ListItem::new(format!("#{} {} {}", e.seq, e.kind, e.payload)))
                .collect();
            f.render_widget(
                List::new(items).block(
                    Block::default()
                        .title("Agent Stream (terminal spine)")
                        .borders(Borders::ALL),
                ),
                chunks[1],
            );
        }
        View::Audit => {
            let items: Vec<ListItem> = snap
                .receipts
                .iter()
                .map(|r| {
                    ListItem::new(format!(
                        "{} task={} kind={} evidence={}",
                        r.id, r.task_id, r.kind, r.evidence
                    ))
                })
                .collect();
            f.render_widget(
                List::new(items).block(
                    Block::default()
                        .title("Audit & Gatekeeper (evidence receipt first)")
                        .borders(Borders::ALL),
                ),
                chunks[1],
            );
        }
        View::Registry => {
            let matrix = studio_core::mcp::derive_tool_map();
            let mut lines: Vec<ListItem> = matrix
                .iter()
                .map(|(role, tools)| ListItem::new(format!("{role}: {}", tools.join(", "))))
                .collect();
            lines.push(ListItem::new("--- token burn ---"));
            for b in &snap.burn {
                lines.push(ListItem::new(format!("{} {}", b.session, b.total)));
            }
            f.render_widget(
                List::new(lines).block(
                    Block::default()
                        .title("MCP Registry (capability matrix + burn)")
                        .borders(Borders::ALL),
                ),
                chunks[1],
            );
        }
    }
    f.render_widget(
        Paragraph::new("tab: switch view · 1-4: jump · q: quit · 100ms CDC poll"),
        chunks[2],
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "studio.db".into());
    let store =
        StateStore::open(&db_path).unwrap_or_else(|_| StateStore::open_in_memory().unwrap());
    let mut terminal = ratatui::init();
    let mut view = View::WarRoom;
    loop {
        let snap = store
            .snapshot()
            .unwrap_or_else(|_| studio_core::projection::Snapshot {
                db_age_ms: 0,
                source: "studio.db".into(),
                fleet: vec![],
                stream: vec![],
                receipts: vec![],
                burn: vec![],
                change_seq: 0,
                runtime_mode: studio_core::projection::runtime_mode_from_env(),
            });
        terminal.draw(|f| render(f, view, &snap))?;
        if event::poll(std::time::Duration::from_millis(POLL_MS))? {
            if let Event::Key(k) = event::read()? {
                match k.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Tab => view = view.next(),
                    KeyCode::Char('1') => view = View::WarRoom,
                    KeyCode::Char('2') => view = View::Stream,
                    KeyCode::Char('3') => view = View::Audit,
                    KeyCode::Char('4') => view = View::Registry,
                    _ => {}
                }
            }
        }
    }
    ratatui::restore();
    Ok(())
}
