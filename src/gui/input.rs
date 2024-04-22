use super::types::{Key, KeyEvent, Modifiers};
use crate::Command;

// Keymap
// `1234567890-=
//  qwertyuiop[]
//  asdfghjkl;'#
//  \zxcvbnm,./

// -- Normal --
// Movement dfjk
// Word: ec
// Paragraph: im
// Insert: l
// Append: o
// Change: .
// Delete: p
// Search: h, space h
// Next result: n, space n
// Select line, grow selection up/down: line: r, space r
// Begin line below/above: g, space g
// Undo/redo: u
// Repeat: ,

// -- Insert --
// Leave: Escape
// Delete word backwards: ctrl+w

enum Mode {
    Normal,
    Insert,
}

pub struct Input {
    mode: Mode,
}

impl Input {
    pub fn new() -> Self {
        Self { mode: Mode::Normal }
    }

    pub fn parse(&self, event: KeyEvent) -> Option<Command> {
        match self.mode {
            Mode::Normal => self.parse_normal(event.key, event.mods),
            Mode::Insert => self.parse_insert(event),
        }
    }

    fn parse_normal(&self, key: Key, mods: Modifiers) -> Option<Command> {
        match (key, mods) {
            (Key::D, Modifiers::empty()) => Some(Command::MoveLeft),
            (Key::F, Modifiers::empty()) => Some(Command::MoveRight),
            (Key::J, Modifiers::empty()) => Some(Command::MoveDown),
            (Key::K, Modifiers::empty()) => Some(Command::MoveUp),
            (Key::L, Modifiers::empty()) => { self.mode = Mode::Insert; None },
            _ => None
        }
    }

    fn parse_insert(&self, event: KeyEvent) -> Option<Command> {
        match event {
            Event { key: Key::Escape, .. } => { self.mode = Mode::Normal; None }
            Event { translated: Some(c), .. } => Some(Insert(c)),
            _ => None
        }
    }
}

// Cursors on client or server?

// enum InputCommand {
// }