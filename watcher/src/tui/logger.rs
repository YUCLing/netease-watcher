use ansi_to_tui::IntoText;
use lazy_static::lazy_static;
use ratatui::text::Text;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};

use super::TUI_NOTIFY;

lazy_static! {
    pub static ref LOG_BUFFER: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    pub static ref LOG_TEXT: Arc<Mutex<Text<'static>>> = Arc::new(Mutex::new(Text::raw("")));
}

pub struct TuiLogger;

unsafe impl Send for TuiLogger {}

impl Write for TuiLogger {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut lck = LOG_BUFFER.lock().unwrap();
        lck.push_str(&String::from_utf8_lossy(buf));
        *LOG_TEXT.lock().unwrap() = lck.into_text().unwrap();
        TUI_NOTIFY.clone().notify_one();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
