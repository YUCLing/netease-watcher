use std::time::Duration;

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::{Line, Span, Text},
    Frame,
};

use crate::{netease::FIND_RETRY_SECS, tui::{TUI_FOUND_CM, TUI_LAST_FIND_TIME}};

const ASCII_ART: &str = r#"  _   _      _                       __        __    _       _               
 | \ | | ___| |_ ___  __ _ ___  ___  \ \      / /_ _| |_ ___| |__   ___ _ __ 
 |  \| |/ _ \ __/ _ \/ _` / __|/ _ \  \ \ /\ / / _` | __/ __| '_ \ / _ \ '__|
 | |\  |  __/ ||  __/ (_| \__ \  __/   \ V  V / (_| | || (__| | | |  __/ |   
 |_| \_|\___|\__\___|\__,_|___/\___|    \_/\_/ \__,_|\__\___|_| |_|\___|_|   
                                                                             "#;
const ASCII_ART_WIDTH: u8 = 78;

const MINI_ASCII_ART: &str = r#"  _   ___        __
 | \ | \ \      / /
 |  \| |\ \ /\ / / 
 | |\  | \ V  V /  
 |_| \_|  \_/\_/   
                   "#;
const MINI_ASCII_ART_WIDTH: u8 = 20;

pub fn render_header(frame: &mut Frame, endpoint: &String, rect: Rect) {
    let found = *TUI_FOUND_CM.lock().unwrap();
    let info_text = Text::from(vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("Server at "),
            Span::raw(endpoint).bold().underlined(),
        ])
        .light_red(),
        Line::from(vec![
            Span::raw("WebSocket at "),
            Span::raw("/ws").bold().underlined(),
            Span::raw(" or any other for HTTP")
        ]).light_magenta(),
        if found {
            Line::raw("Found Netease Cloud Music").green()
        } else {
            let duration = Duration::from_secs(FIND_RETRY_SECS);
            let elapsed = TUI_LAST_FIND_TIME.lock().unwrap().elapsed();
            Line::raw(format!("Next try to find Cloud Music in {:.1} secs", duration.checked_sub(elapsed).unwrap_or(Duration::ZERO).as_secs_f32())).red().bold()
        },
        Line::raw(format!("v{} by YUCLing@GitHub", env!("CARGO_PKG_VERSION"))).black(),
    ])
        .on_white();
    let full_width = info_text.width() + ASCII_ART_WIDTH as usize;
    let mini = rect.width as usize - 1 < full_width; // art itself contains left 1 margin
    let layout = Layout::new(
        ratatui::layout::Direction::Horizontal,
        [
            Constraint::Length(if mini { MINI_ASCII_ART_WIDTH } else { ASCII_ART_WIDTH } as u16),
            Constraint::Fill(1),
        ],
    )
    .split(rect);
    frame.render_widget(
        Text::raw(if mini { MINI_ASCII_ART } else { ASCII_ART })
            .red()
            .on_white(),
        layout[0],
    );
    frame.render_widget(
        info_text,
        layout[1],
    );
}
