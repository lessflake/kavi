use super::types::Message;
use anyhow::Context as _;
use serde::Serialize;
use std::{
    io::{BufRead, BufReader, Write},
    process::{ChildStderr, ChildStdin, ChildStdout},
};

#[allow(dead_code)]
pub struct Transport {
    thread: Option<std::thread::JoinHandle<()>>,
    stdin: ChildStdin,
    _stderr: ChildStderr,
}

// impl Drop for Transport {
//     fn drop(&mut self) {
//         match self.thread.take() {
//             Some(thread) => thread.join().unwrap(),
//             None => {}
//         }
//     }
// }

impl Transport {
    pub fn start(
        stdin: ChildStdin,
        stdout: ChildStdout,
        stderr: ChildStderr,
    ) -> (Self, crossbeam_channel::Receiver<Message>) {
        let mut stdout = BufReader::new(stdout);
        let (tx, rx) = crossbeam_channel::unbounded();

        let thread = std::thread::Builder::new()
            .name("lsp listener".to_string())
            .spawn(move || loop {
                let msg = match try_read(&mut stdout) {
                    Ok(msg) => msg,
                    Err(e) => {
                        log::warn!("lsp: stdout closed ({})", e);
                        break;
                    }
                };

                tx.send(msg).expect("where did channel go?");
            })
            .unwrap();

        (
            Self {
                thread: Some(thread),
                stdin,
                _stderr: stderr,
            },
            rx,
        )
    }

    pub fn send(&mut self, message: Message) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct JsonRpc {
            jsonrpc: &'static str,
            #[serde(flatten)]
            message: Message,
        }

        let text = serde_json::to_string(&JsonRpc {
            jsonrpc: "2.0",
            message,
        })?;

        self.stdin
            .write_all(format!("Content-Length: {}\r\n\r\n", text.len()).as_bytes())?;
        self.stdin.write_all(text.as_bytes())?;
        self.stdin.flush()?;

        Ok(())
    }
}

fn try_read<R>(reader: &mut R) -> anyhow::Result<Message>
where
    R: BufRead,
{
    let mut buf = String::new();
    let mut size = None;
    loop {
        buf.clear();

        if reader.read_line(&mut buf)? == 0 {
            anyhow::bail!("stream closed");
        }
        assert!(buf.ends_with("\r\n"));

        let header = buf.trim();
        if header.is_empty() {
            break;
        }

        let parts = header.split_once(": ");
        match parts {
            Some(("Content-Length", value)) => {
                size = Some(value.parse::<usize>().context("invalid content length")?);
            }
            Some((_, _)) => {}
            None => {}
        }
    }

    let size = size.context("missing context length")?;

    let mut buf = buf.into_bytes();
    buf.resize(size, 0);
    reader.read_exact(&mut buf)?;
    let buf = String::from_utf8(buf).context("server sent invalid utf8")?;
    Ok(serde_json::from_str::<Message>(&buf)?)
}
