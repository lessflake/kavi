mod transport;
mod types;

use anyhow::Context as _;
use serde::Serialize;
use std::{
    borrow::Cow,
    process::{Child, Command, Stdio},
};
use transport::Transport;
use types::{ErrorCode, Message, Notification, Request, Response};

const LSP_PATH: &str = "";

type ResponseCallback<T> = Box<dyn FnOnce(&mut Client, anyhow::Result<T>) + Send + Sync>;

#[allow(dead_code)]
pub struct Lsp {
    thread: Option<std::thread::JoinHandle<()>>,
    pub client: Client,
}

// TODO this isn't good enough to prevent random weird crash on exit 100% of the time
// waiting on the listener thread to join means waiting for the lsp to fully shutdown,
// closing its socket, which takes too long to feel viable.
// impl Drop for Lsp {
//     fn drop(&mut self) {
//         let _ = self.client.sender.send(ClientMessage::Shutdown);
//         match self.thread.take() {
//             Some(thread) => thread.join().unwrap(),
//             None => {}
//         }
//     }
// }

pub fn start() -> anyhow::Result<(Lsp, crossbeam_channel::Receiver<Message>)> {
    let mut process = Command::new(LSP_PATH)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdin = process.stdin.take().unwrap();
    let stdout = process.stdout.take().unwrap();
    let stderr = process.stderr.take().unwrap();
    let (transport, server_rx) = Transport::start(stdin, stdout, stderr);
    let (tx, client_rx) = crossbeam_channel::unbounded::<ClientMessage>();
    let (client_tx, rx) = crossbeam_channel::unbounded::<Message>();

    let client = Client { sender: tx };

    let mut ctx = Context {
        _process: process,
        transport,
        handle: client.clone(),
        request_counter: 0,
        pending_requests: Vec::new(),
    };

    let thread = std::thread::Builder::new()
        .name("lsp listener".to_string())
        .spawn(move || loop {
            crossbeam_channel::select! {
                recv(client_rx) -> msg => match ctx.process_client_message(msg.unwrap()).unwrap() {
                    true => {}
                    false => break,
                },
                recv(server_rx) -> msg => match msg {
                    Ok(msg) => ctx.process_server_message(&client_tx, msg).unwrap(),
                    Err(_) => break,
                }
            }
        })
        .context("failed to spawn lsp thread")?;

    let lsp = Lsp {
        thread: Some(thread),
        client,
    };

    Ok((lsp, rx))
}

#[derive(Clone)]
pub struct Client {
    sender: crossbeam_channel::Sender<ClientMessage>,
}

impl Client {
    pub fn request<R, F>(&mut self, params: R::Params, callback: F) -> anyhow::Result<()>
    where
        R: lsp_types::request::Request,
        R::Params: Serialize,
        F: Send + Sync + FnOnce(&mut Self, anyhow::Result<R::Result>) + 'static,
    {
        let method = R::METHOD.into();
        let params = serde_json::to_value(params)?;

        let callback = Box::new(
            move |ctx: &mut Client, response: anyhow::Result<serde_json::Value>| {
                let response = response.map(|res| serde_json::from_value(res).unwrap());
                callback(ctx, response);
            },
        );

        let msg = ClientMessage::Request {
            method,
            params,
            callback,
        };

        self.sender.send(msg)?;
        Ok(())
    }

    pub fn notify<N: lsp_types::notification::Notification>(
        &mut self,
        params: N::Params,
    ) -> anyhow::Result<()>
    where
        N::Params: Serialize,
    {
        let method = N::METHOD.into();
        let params = serde_json::to_value(params)?;
        let notification = Notification { method, params };
        self.sender
            .send(ClientMessage::Notification(notification))?;
        Ok(())
    }

    pub fn respond<R: lsp_types::request::Request>(
        &mut self,
        id: u64,
        result: R::Result,
    ) -> anyhow::Result<()> {
        let result = serde_json::to_value(result)?;
        let response = Response {
            id,
            result: Some(result),
            error: None, // TODO allow sending errors? not sure if useful to client though
        };
        self.sender.send(ClientMessage::Response(response))?;
        Ok(())
    }

    pub fn initialize(&mut self) -> anyhow::Result<()> {
        #[allow(deprecated)]
        let initialize = lsp_types::InitializeParams {
            process_id: None,
            root_path: None,
            root_uri: None,
            initialization_options: None,
            capabilities: lsp_types::ClientCapabilities {
                workspace: None,
                text_document: None,
                window: None,
                general: None,
                experimental: None,
            },
            trace: None,
            workspace_folders: None,
            client_info: None,
            locale: None,
        };

        self.request::<lsp_types::request::Initialize, _>(initialize, |ctx, resp| {
            let lsp_types::InitializeResult {
                capabilities: _capabilities,
                ..
            } = resp.unwrap();
            // println!("{:#?}", capabilities);
            ctx.notify::<lsp_types::notification::Initialized>(lsp_types::InitializedParams {})
                .unwrap();
        })?;

        Ok(())
    }

    pub fn shutdown(&mut self) -> anyhow::Result<()> {
        self.request::<lsp_types::request::Shutdown, _>((), |ctx, _| {
            ctx.notify::<lsp_types::notification::Exit>(()).unwrap();
        })?;
        Ok(())
    }
}

struct Context {
    pub _process: Child,
    transport: Transport,
    handle: Client,
    request_counter: u64,
    pending_requests: Vec<(u64, ResponseCallback<serde_json::Value>)>,
}

impl Context {
    fn next_request_id(&mut self) -> u64 {
        let id = self.request_counter;
        self.request_counter += 1;
        id
    }

    fn process_client_message(&mut self, msg: ClientMessage) -> anyhow::Result<bool> {
        match msg {
            ClientMessage::Request {
                method,
                params,
                callback,
            } => {
                let id = self.next_request_id();
                let request = Request { id, method, params };
                self.pending_requests.push((id, callback));
                self.transport.send(Message::Request(request))?;
            }
            ClientMessage::Response(resp) => self.transport.send(Message::Response(resp))?,
            ClientMessage::Notification(notif) => {
                self.transport.send(Message::Notification(notif))?
            }
            ClientMessage::Shutdown => return Ok(false),
        }

        Ok(true)
    }

    fn process_server_message(
        &mut self,
        client_tx: &crossbeam_channel::Sender<Message>,
        msg: Message,
    ) -> anyhow::Result<()> {
        match msg {
            Message::Request(_) | Message::Notification(_) => {
                client_tx.send(msg)?;
            }
            Message::Response(resp) => {
                let result = if let Some(err) = resp.error {
                    Err(anyhow::anyhow!(
                        "{} {:?} {} - data: {:?}",
                        err.code,
                        ErrorCode::from(err.code),
                        err.message,
                        err.data
                    ))
                } else {
                    Ok(resp.result.unwrap_or(serde_json::Value::Null))
                };

                let (_, callback) = self
                    .pending_requests
                    .binary_search_by_key(&resp.id, |(id, _)| *id)
                    .map_err(|_| anyhow::anyhow!("no matching request id for response"))
                    .map(|index| self.pending_requests.remove(index))?;

                callback(&mut self.handle, result);
            }
        }

        Ok(())
    }
}

#[allow(dead_code)]
enum ClientMessage {
    Shutdown,
    Request {
        method: Cow<'static, str>,
        params: serde_json::Value,
        callback: ResponseCallback<serde_json::Value>,
    },
    Notification(Notification),
    Response(Response),
}
