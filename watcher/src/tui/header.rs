use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::{Line, Span, Text},
    Frame,
};

const ASCII_ART: &str = r#"  _   _      _                       __        __    _       _               
 | \ | | ___| |_ ___  __ _ ___  ___  \ \      / /_ _| |_ ___| |__   ___ _ __ 
 |  \| |/ _ \ __/ _ \/ _` / __|/ _ \  \ \ /\ / / _` | __/ __| '_ \ / _ \ '__|
 | |\  |  __/ ||  __/ (_| \__ \  __/   \ V  V / (_| | || (__| | | |  __/ |   
 |_| \_|\___|\__\___|\__,_|___/\___|    \_/\_/ \__,_|\__\___|_| |_|\___|_|   
                                                                             "#;

const MINI_ASCII_ART: &str = r#"  _   ___        __
 | \ | \ \      / /
 |  \| |\ \ /\ / / 
 | |\  | \ V  V /  
 |_| \_|  \_/\_/   
                   "#;

pub fn render_header(frame: &mut Frame, endpoint: &String, rect: Rect) {
    let mini = rect.width < 125;
    let layout = Layout::new(
        ratatui::layout::Direction::Horizontal,
        [
            Constraint::Length(if mini { 20 } else { 78 }),
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
        Text::from(vec![
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
            Line::raw(format!("v{}", env!("CARGO_PKG_VERSION"))).black(),
            Line::raw("by YUCLing@GitHub").black(),
        ])
        .on_white(),
        layout[1],
    );
}
