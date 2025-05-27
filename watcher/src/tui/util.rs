use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span, StyledGrapheme},
    Frame,
};

pub fn format_seconds_to_hhmm(seconds: i64) -> String {
    let minutes = seconds / 60;
    let seconds = seconds - minutes * 60;

    format!("{:02}:{:02}", minutes, seconds)
}

pub fn in_rect(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

pub fn render_progress_bar(frame: &mut Frame, line: Line, progress: f64, rect: Rect) {
    let width = rect.width as usize;
    let ident_width = width.saturating_sub(line.width()) / 2;
    let played_width = ((width as f64) * progress) as usize;
    let graphemes: Vec<ratatui::text::StyledGrapheme<'_>> =
        line.styled_graphemes(Style::default()).collect();
    let space = StyledGrapheme::new(" ", Style::default());
    for i in 0..width {
        let grapheme = if i >= ident_width && i < ident_width + graphemes.len() {
            &graphemes[i - ident_width]
        } else {
            &space
        };
        let mut span = Span::raw(grapheme.symbol).style(grapheme.style);
        if i < played_width {
            span = span.white().on_blue();
        } else {
            span = span.gray().on_cyan();
        }
        frame.render_widget(span, Rect::new(rect.x + i as u16, rect.y, 1, 1));
    }
}
