use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use lazy_static::lazy_static;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};
use tokio::sync::Notify;

use crate::Music;

mod header;
pub mod logger;
mod util;

lazy_static! {
    pub static ref TUI_NOTIFY: Arc<Notify> = Arc::new(Notify::new());
    pub static ref TUI_MUSIC: Arc<Mutex<Option<Music>>> = Arc::new(Mutex::new(None));
    pub static ref TUI_MUSIC_TIME: Arc<Mutex<f64>> = Arc::new(Mutex::new(0.));
    pub static ref TUI_NEXT_FIND_TIME: Arc<Mutex<Option<Instant>>> =
        Arc::new(Mutex::new(Some(Instant::now())));
}

struct State<'a> {
    endpoint: &'a String,
    log_scroll_state: ScrollbarState,
    log_scroll: usize,
}

#[derive(Default)]
struct RenderedState {
    log_area_height: u16,
    total_log_lines: usize,
    exit_button_area: Rect,
}

fn render(frame: &mut Frame, state: &mut State, rendered_state: &mut RenderedState) {
    let layout = Layout::new(
        ratatui::layout::Direction::Vertical,
        [
            Constraint::Length(6),
            Constraint::Fill(1),
            Constraint::Length(1),
        ],
    )
    .split(frame.area());
    header::render_header(frame, state.endpoint, layout[0]);
    {
        // auto scroll:
        // 1. end of log was inside viewport
        // 2. new log size exceeds viewport
        let auto_scroll_pre_cond = state.log_scroll < rendered_state.total_log_lines
            && rendered_state.total_log_lines
                <= state.log_scroll + rendered_state.log_area_height as usize;
        rendered_state.log_area_height = layout[1].height;
        let paragraph = {
            let buf = logger::LOG_TEXT.lock().unwrap();
            let p = Paragraph::new(buf.clone()).wrap(Wrap { trim: true });
            rendered_state.total_log_lines = p.line_count(layout[1].width - 1);
            if auto_scroll_pre_cond
                && rendered_state.total_log_lines > state.log_scroll + layout[1].height as usize
            {
                // auto scroll to make sure last log item is at the bottom of viewport
                state.log_scroll = rendered_state
                    .total_log_lines
                    .saturating_sub(layout[1].height as usize);
                state.log_scroll_state = state.log_scroll_state.position(state.log_scroll);
            }
            state.log_scroll_state = state
                .log_scroll_state
                .content_length(rendered_state.total_log_lines)
                .viewport_content_length(layout[1].height as usize);
            p.block(Block::new().borders(Borders::RIGHT))
                .scroll((state.log_scroll as u16, 0))
        };
        frame.render_widget(paragraph, layout[1]);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            layout[1],
            &mut state.log_scroll_state,
        );
    }
    {
        let mut music_info_line = Line::raw(" ").black().on_white();
        let mut progress_line = Line::raw("");

        let mut progress = 0.;

        {
            let music = TUI_MUSIC.lock().unwrap();

            if let Some(music) = music.as_ref() {
                music_info_line.push_span(music.name.clone());
                if let Some(aliases) = &music.aliases {
                    music_info_line
                        .push_span(Span::raw(format!(" [{}]", aliases.join("/"))).dark_gray());
                }
                music_info_line.push_span(" - ");
                music_info_line.push_span(music.artists.join(", ").clone());
                music_info_line
                    .push_span(Span::raw(format!(" ({})", music.id)).dark_gray().italic());

                let current_time = *TUI_MUSIC_TIME.lock().unwrap() as i64;
                let total_duration = music.duration / 1000;
                progress = current_time as f64 / total_duration as f64;

                progress_line
                    .push_span(Span::raw(util::format_seconds_to_hhmm(current_time)).italic());
                progress_line.push_span(Span::raw(" / "));
                progress_line
                    .push_span(Span::raw(util::format_seconds_to_hhmm(total_duration)).bold());
            } else {
                music_info_line.push_span(Span::raw("no music").italic().bold());
            }
            music_info_line.push_span(" ");
        }

        let esc_line = Line::from(vec![Span::raw(" ESC ").on_green(), Span::raw(" Exit ")])
            .black()
            .on_white();

        let progress_line_width = progress_line.width();
        let layout = Layout::new(
            ratatui::layout::Direction::Horizontal,
            [
                Constraint::Max(music_info_line.width() as u16),
                Constraint::Min(progress_line_width as u16),
                Constraint::Length(esc_line.width() as u16),
            ],
        )
        .spacing(2)
        .split(layout[2]);

        frame.render_widget(music_info_line, layout[0]);
        util::render_progress_bar(frame, progress_line, progress, layout[1]);
        frame.render_widget(esc_line, layout[2]);
        rendered_state.exit_button_area = layout[2];
    }
}

fn scroll_down(state: &mut State, rendered_state: &mut RenderedState) {
    let next_scroll = state.log_scroll.saturating_add(1);
    if next_scroll < rendered_state.total_log_lines {
        state.log_scroll = next_scroll;
        state.log_scroll_state = state.log_scroll_state.position(state.log_scroll);
    }
}
fn scroll_up(state: &mut State) {
    state.log_scroll = state.log_scroll.saturating_sub(1);
    state.log_scroll_state = state.log_scroll_state.position(state.log_scroll);
}

pub async fn run(endpoint: String) {
    let mut state = State {
        endpoint: &endpoint,
        log_scroll_state: Default::default(),
        log_scroll: 0,
    };
    let mut rendered_state = RenderedState::default();

    let mut terminal = ratatui::init();

    if let Err(_err) = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)
    {
        log::error!("Unable to enable mouse events.");
    }

    // helper state for making sure only exit when a complete cycle of mouse down and up is done
    let mut exit_btn_hold = false;
    let notify = TUI_NOTIFY.clone();
    loop {
        use crossterm::event::{
            self, Event, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind,
        };

        terminal
            .draw(|f| render(f, &mut state, &mut rendered_state))
            .unwrap();
        async fn poller() -> std::io::Result<Event> {
            let notify = TUI_NOTIFY.clone();
            loop {
                if event::poll(Duration::from_secs(0)).unwrap() {
                    return event::read();
                } else {
                    // no term event yet, see if other things need a update.

                    // finder countdown updating
                    if TUI_NEXT_FIND_TIME.lock().unwrap().is_some() {
                        notify.notify_one();
                    }

                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        tokio::select! {
            event = poller() => {
                let event = event.unwrap();
                match event {
                    Event::Key(KeyEvent { code, modifiers: _, kind, state: _ }) => {
                        match code {
                            KeyCode::Down if kind == KeyEventKind::Press => {
                                scroll_down(&mut state, &mut rendered_state);
                            },
                            KeyCode::Up if kind == KeyEventKind::Press => {
                                scroll_up(&mut state);
                            },
                            KeyCode::Esc => {
                                break;
                            },
                            _ => {}
                        }
                    }
                    Event::Mouse(MouseEvent { kind, column, row, modifiers: _ }) => {
                        match kind {
                            MouseEventKind::Down(event::MouseButton::Left) => {
                                if util::in_rect(rendered_state.exit_button_area, column, row) {
                                    exit_btn_hold = true;
                                }
                            }
                            MouseEventKind::Up(event::MouseButton::Left) => {
                                if util::in_rect(rendered_state.exit_button_area, column, row) && exit_btn_hold {
                                    break;
                                }
                                exit_btn_hold = false;
                            }
                            MouseEventKind::ScrollDown => {
                                scroll_down(&mut state, &mut rendered_state);
                            }
                            MouseEventKind::ScrollUp => {
                                scroll_up(&mut state);
                            }
                            _ => {}
                        }
                    }
                    Event::Resize(_w, _h) => {
                        if state.log_scroll >= rendered_state.total_log_lines {
                            state.log_scroll = rendered_state.total_log_lines.saturating_sub(1);
                            state.log_scroll_state = state.log_scroll_state.position(state.log_scroll);
                        }
                    }
                    _ => {}
                }
            }
            _ = notify.notified() => {}
        };
    }
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    ratatui::restore();
}
