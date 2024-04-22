use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Notification(Notification),
    Response(Response),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Notification {
    pub method: Cow<'static, str>,
    #[serde(default = "serde_json::Value::default")]
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub params: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Request {
    pub id: u64,
    pub method: Cow<'static, str>,
    #[serde(default = "serde_json::Value::default")]
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub params: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Clone, Copy, Debug)]
pub enum ErrorCode {
    ParseError = -32700,
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,
    InternalError = -32603,
    ServerErrorStart = -32099,
    ServerErrorEnd = -32000,
    ServerNotInitialized = -32002,
    Unknown = -32001,
    RequestCanceled = -32800,
    ContentModified = -32801,
    ServerCancelled = -32802,
}

impl From<i32> for ErrorCode {
    fn from(code: i32) -> Self {
        match code {
            -32700 => Self::ParseError,
            -32600 => Self::InvalidRequest,
            -32601 => Self::MethodNotFound,
            -32602 => Self::InvalidParams,
            -32603 => Self::InternalError,
            -32099 => Self::ServerErrorStart,
            -32000 => Self::ServerErrorEnd,
            -32002 => Self::ServerNotInitialized,
            -32001 => Self::Unknown,
            -32800 => Self::RequestCanceled,
            -32801 => Self::ContentModified,
            -32802 => Self::ServerCancelled,
            _ => Self::Unknown,
        }
    }
}
