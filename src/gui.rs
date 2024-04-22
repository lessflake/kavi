mod input;
mod render;
mod types;
mod window;

use crate::{ClientMessage, Command, ServerMessage};
use crossbeam_channel::{Receiver, Sender};
use render::Render;
use types::{Key, KeyEvent, KeyState};
use window::{Window, WindowEvent};

// What do we want from our UI?
// - Splits
// - Floating windows w/ arbitrary text

// How is rendering going to work?
// 1. Push data to GPU
// 2. Compute pipeline renders glyphs to a buffer per "window"
// 3. Graphics pipeline composits these buffers

// How we going to transform a rope into GPU friendly data?
// - Try sparse since already did dense last time
// - Map buffer, slice length size. Write entire slice. Unmap buffer.

// How compositing going to work?
// - Quad for each window
// - Render the relevant texture from compute shader to quad

// Separation of concerns per module, bottom up:
// vulkan: Basic wrappers over high level Vulkan structures
// backend: Vulkan abstraction layer - APIs for pipelines, command buffers, descriptor sets?
// render: Application-specific structures built with backend abstractions

pub struct Gui {
    thread: Option<std::thread::JoinHandle<()>>,
    pub tx: Sender<ServerMessage>,
    pub rx: Receiver<ClientMessage>,
}

impl Drop for Gui {
    fn drop(&mut self) {
        if let Some(handle) = self.thread.take() {
            handle.join().unwrap()
        }
    }
}

pub fn spawn() -> anyhow::Result<Gui> {
    let (server_tx, rx) = crossbeam_channel::unbounded();
    let (tx, server_rx) = crossbeam_channel::unbounded();
    let thread = std::thread::Builder::new()
        .name("Client".to_string())
        .spawn(move || run(tx, rx))?;

    Ok(Gui {
        thread: Some(thread),
        tx: server_tx,
        rx: server_rx,
    })
}

struct App {
    window: Window,
    render: Render,
    tx: Sender<ClientMessage>,
    rx: Receiver<ServerMessage>,
    text: ropey::Rope,
}

fn run(tx: Sender<ClientMessage>, rx: Receiver<ServerMessage>) {
    let window = Window::start_with_thread(1280, 720).unwrap();
    let mut render = Render::new(&window).unwrap();
    let text = ropey::Rope::new();

    match render.draw_frame(&text) {
        Ok(_) => {}
        Err(_) => panic!(),
    }

    let mut app = App {
        window,
        render,
        tx,
        rx,
        text,
    };

    'main_loop: loop {
        crossbeam_channel::select! {
            recv(app.rx) -> msg => {
                log::trace!("client: received {:?}", msg);
            }
            recv(app.window.rx) -> event => match event {
                Ok(event) => if app.handle_window_event(event) { break 'main_loop },
                Err(e) => log::error!("server closed channel ({e})"),
            }
        }
    }

    app.shutdown().unwrap();
}

impl App {
    fn handle_window_event(self: &mut Self, event: WindowEvent) -> bool {
        match event {
            WindowEvent::Keyboard(event) => self.handle_keyboard_event(event).unwrap(),
            WindowEvent::Quit => return true,
            WindowEvent::Resize(width, height, tx) => {
                self.render.resize(width, height).unwrap();
                self.render.draw_frame(&self.text).unwrap();
                if let Some(tx) = tx {
                    tx.send(()).unwrap()
                };
            }
            _ => {}
        };

        false
    }

    fn handle_keyboard_event(&mut self, event: KeyEvent) -> anyhow::Result<()> {
        match event {
            KeyEvent {
                key: Key::Escape, ..
            } => self.window.close(),
            KeyEvent {
                key: Key::F11,
                state: KeyState::Press,
                ..
            } => self.window.toggle_fullscreen(),
            KeyEvent {
                key: Key::Backspace,
                state: KeyState::Press,
                ..
            } => {
                self.command(Command::Delete)?;
                // if self.text.len_chars() > 0 {
                //     self.text.remove(self.text.len_chars() - 1..);
                //     self.render.draw_frame(&self.text)?;
                // }
            }
            KeyEvent {
                key: Key::Return,
                state: KeyState::Press,
                ..
            } => {
                self.command(Command::NewLine)?;
                // self.text.insert_char(self.text.len_chars(), '\n');
                // self.render.draw_frame(&self.text)?;
            }
            KeyEvent {
                translated: Some(c),
                // key: Key::F11,
                state: KeyState::Press,
                ..
            } => {
                self.command(Command::Insert(c))?;
                // self.text.insert_char(self.text.len_chars(), c);
                // self.render.draw_frame(&self.text)?;
            }
            // KeyEvent {
            //     key: Key::A,
            //     state: KeyState::Press,
            //     ..
            // } => {
            //     self.render.draw_frame(&self.text)?;
            // }
            _ => {}
        };

        Ok(())
    }

    fn command(&self, command: Command) -> anyhow::Result<()> {
        Ok(self.tx.send(ClientMessage::Command(command))?)
    }

    fn shutdown(&self) -> anyhow::Result<()> {
        Ok(self.tx.send(ClientMessage::Shutdown)?)
    }
}
