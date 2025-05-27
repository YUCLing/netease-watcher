use std::panic;

use fern::colors::{Color, ColoredLevelConfig};

pub fn setup_logger() -> Result<(), fern::InitError> {
    let colors = ColoredLevelConfig::new().info(Color::BrightBlue);
    fern::Dispatch::new()
        .format(move |out, message, record| {
            let time = chrono::Local::now();
            out.finish(format_args!(
                "[{} {}] {}",
                time.format("%H:%M:%S"),
                colors.color(record.level()),
                message
            ));
        })
        .chain({
            let dispatcher = fern::Dispatch::new()
                .level(log::LevelFilter::Warn)
                .level_for("netease_watcher", log::LevelFilter::Debug);
            #[cfg(feature = "tui")]
            {
                dispatcher.chain(Into::<fern::Output>::into(
                    Box::new(crate::tui::logger::TuiLogger) as Box<dyn Send + std::io::Write>,
                ))
            }
            #[cfg(not(feature = "tui"))]
            {
                dispatcher.chain(std::io::stdout())
            }
        })
        .apply()?;
    Ok(())
}

pub fn setup_panic_logger_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let error_with_message = |str: &String| {
            log::error!(
                "Panic occurred at {}: {}",
                info.location()
                    .map(|x| x.to_string())
                    .unwrap_or("unknown".to_string()),
                str
            );
        };
        let payload = info.payload();
        if let Some(msg) = payload.downcast_ref::<&str>() {
            error_with_message(&msg.to_string());
        } else if let Some(msg) = payload.downcast_ref::<String>() {
            error_with_message(msg);
        } else {
            log::error!(
                "Panic occurred at {}",
                info.location()
                    .map(|x| x.to_string())
                    .unwrap_or("unknown".to_string())
            );
        }
        // still calls the default hook for detailed information.
        default_hook(info);
    }));
}
