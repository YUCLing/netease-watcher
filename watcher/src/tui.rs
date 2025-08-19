use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use lazy_static::lazy_static;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
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
    pub static ref TUI_FOUND_CM: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    pub static ref TUI_LAST_FIND_TIME: Arc<Mutex<Instant>> = Arc::new(Mutex::new(Instant::now()));
}

struct State<'a> {
    endpoint: &'a String,
    scroll_offset: u16,
}

#[derive(Default)]
struct RenderedState {
    log_area_height: u16,
    total_log_lines: usize,
    exit_button_area: Rect,
}

fn render(frame: &mut Frame, state: &State, rendered_state: &mut RenderedState) {
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
        let mut paragraph = {
            let buf = logger::LOG_TEXT.lock().unwrap();
            Paragraph::new(buf.clone()).wrap(Wrap { trim: true })
        };
        let line_count = paragraph.line_count(layout[1].width);
        if state.scroll_offset == u16::MAX {
            // stick to bottom
            if line_count as u16 > layout[1].height {
                let offset = line_count as u16 - layout[1].height;
                paragraph = paragraph.scroll((offset, 0));
            }
        } else {
            paragraph = paragraph.scroll((state.scroll_offset, 0));
        }
        rendered_state.log_area_height = layout[1].height;
        rendered_state.total_log_lines = line_count;
        frame.render_widget(paragraph, layout[1]);
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

pub async fn run(endpoint: String) {
    let mut state = State {
        endpoint: &endpoint,
        scroll_offset: u16::MAX,
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
        use crossterm::event::{self, Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind};

        terminal
            .draw(|f| render(f, &state, &mut rendered_state))
            .unwrap();
        async fn poller() -> std::io::Result<Event> {
            let notify = TUI_NOTIFY.clone();
            loop {
                if event::poll(Duration::from_secs(0)).unwrap() {
                    return event::read();
                } else {
                    // no term event yet, see if other things need a update.
 
                    // finder countdown updating
                    if !*TUI_FOUND_CM.lock().unwrap() {
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
                    Event::Key(KeyEvent { code, modifiers: _, kind: _, state: _ }) => {
                        if code == KeyCode::Esc {
                            break;
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
                                if state.scroll_offset == u16::MAX {
                                    // it's at the bottom, don't scroll down
                                    continue;
                                }
                                if rendered_state.total_log_lines >= (state.scroll_offset + rendered_state.log_area_height) as usize {
                                    // not at the bottom and it's overflowing
                                    state.scroll_offset += 1;
                                } else {
                                    // at the bottom, make it stick to bottom
                                    state.scroll_offset = u16::MAX;
                                }
                            }
                            MouseEventKind::ScrollUp => {
                                if state.scroll_offset > 0 {
                                    // not at the top
                                    if state.scroll_offset == u16::MAX {
                                        // we're at the bottom
                                        if rendered_state.total_log_lines > rendered_state.log_area_height as usize {
                                            // make sure we have enough space to engage a scroll up
                                            state.scroll_offset = rendered_state.total_log_lines as u16 - rendered_state.log_area_height;
                                        }
                                    } else {
                                        // just simply scroll up
                                        state.scroll_offset -= 1;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Event::Resize(_w, _h) => {
                        // reset the scrollbar to the bottom. we will need a better handling of this.
                        state.scroll_offset = u16::MAX;
                    }
                    _ => {}
                }
            }
            _ = notify.notified() => {}
        };
    }
    ratatui::restore();
}
