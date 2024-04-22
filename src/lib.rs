#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

pub mod gui;
pub mod lsp;

#[derive(Debug)]
pub enum ServerMessage {}

#[derive(Debug)]
pub enum ClientMessage {
    Command(Command),
    Shutdown,
}

#[derive(Debug)]
pub enum Command {
    Open(String),
    Select(Scope),
    Insert(char),
    Delete,

    NewLine,

    Undo,
    Redo,
}

#[derive(Debug)]
pub enum Scope {
    Character,
    Line,
}
