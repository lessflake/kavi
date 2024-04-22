// use kavi::gui::{self, Key, KeyEvent, Window, WindowEvent};
use kavi::{gui, lsp, ClientMessage};

mod logging {
    use log::{Level, Metadata, Record};

    pub static LOGGER: Logger = Logger;
    pub struct Logger;

    pub fn init(filter: log::LevelFilter) {
        log::set_logger(&LOGGER)
            .map(|_| log::set_max_level(filter))
            .expect("failed to set logger");
    }

    impl log::Log for Logger {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= Level::Trace
        }

        fn log(&self, record: &Record) {
            if self.enabled(record.metadata()) {
                let color = match record.level() {
                    Level::Error => "\x1b[31m",
                    Level::Warn => "\x1b[33m",
                    Level::Info => "\x1b[34m",
                    Level::Debug => "\x1b[32m",
                    Level::Trace => "\x1b[90m",
                };
                println!("{}{:5}\x1b[0m {}", color, record.level(), record.args());
            }
        }

        fn flush(&self) {}
    }
}

fn main() -> anyhow::Result<()> {
    logging::init(log::LevelFilter::Trace);

    // let (mut lsp, lsp_rx) = lsp::start()?;
    // lsp.client.initialize()?;
    let gui = gui::spawn()?;

    loop {
        crossbeam_channel::select! {
            // recv(lsp_rx) -> msg => {
                // log::info!("from lsp: {:?}", msg);
            // }
            recv(gui.rx) -> msg => {
                log::info!("from gui: {:?}", msg);
                match msg.unwrap() {
                    ClientMessage::Shutdown => break,
                    _ => {}
                }
            }
        }
    }

    // lsp.client.shutdown()?;
    Ok(())
}
