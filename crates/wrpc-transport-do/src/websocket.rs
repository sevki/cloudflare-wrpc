//! WebSocket transport for wRPC communication between Durable Objects
//!
//! This module provides WebSocket-based transport for wRPC, enabling
//! bidirectional, persistent connections between Durable Objects.

use futures_util::StreamExt;
use serde::{de::DeserializeOwned, Serialize};
use worker::{WebSocket, WebSocketPair, WebsocketEvent};

use crate::message::{WrpcEnvelope, WrpcRequest, WrpcResponse};
use crate::{Error, Result};

/// WebSocket-based wRPC client for persistent connections
pub struct WrpcWebSocket {
    socket: WebSocket,
}

impl WrpcWebSocket {
    /// Create a new wRPC WebSocket wrapper from an existing WebSocket
    pub fn new(socket: WebSocket) -> Self {
        Self { socket }
    }

    /// Accept the WebSocket connection
    pub fn accept(&self) -> Result<()> {
        self.socket
            .accept()
            .map_err(|e| Error::Transport(e.to_string()))
    }

    /// Send a wRPC request over the WebSocket
    pub fn send_request(&self, request: &WrpcRequest) -> Result<()> {
        let envelope = WrpcEnvelope::request(request)?;
        let json = serde_json::to_string(&envelope)?;
        self.socket
            .send_with_str(&json)
            .map_err(|e| Error::Transport(e.to_string()))
    }

    /// Send a wRPC response over the WebSocket
    pub fn send_response(&self, response: &WrpcResponse) -> Result<()> {
        let envelope = WrpcEnvelope::response(response)?;
        let json = serde_json::to_string(&envelope)?;
        self.socket
            .send_with_str(&json)
            .map_err(|e| Error::Transport(e.to_string()))
    }

    /// Invoke a function and wait for the response
    pub async fn invoke<P, R>(&self, instance: &str, function: &str, params: P) -> Result<R>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let request = WrpcRequest::new(instance, function, params)?;
        self.send_request(&request)?;

        // Wait for response
        let mut event_stream = self
            .socket
            .events()
            .map_err(|e| Error::Transport(e.to_string()))?;

        while let Some(event) = event_stream.next().await {
            match event.map_err(|e| Error::Transport(e.to_string()))? {
                WebsocketEvent::Message(msg) => {
                    if let Some(text) = msg.text() {
                        let envelope: WrpcEnvelope = serde_json::from_str(&text)?;
                        let response: WrpcResponse = serde_json::from_value(envelope.payload)?;
                        return response.decode();
                    }
                }
                WebsocketEvent::Close(_) => {
                    return Err(Error::Transport("WebSocket closed".to_string()));
                }
            }
        }

        Err(Error::Transport("No response received".to_string()))
    }

    /// Send a raw string message
    pub fn send_raw(&self, message: &str) -> Result<()> {
        self.socket
            .send_with_str(message)
            .map_err(|e| Error::Transport(e.to_string()))
    }

    /// Close the WebSocket connection
    pub fn close(&self) -> Result<()> {
        self.socket
            .close::<&str>(None, None)
            .map_err(|e| Error::Transport(e.to_string()))
    }

    /// Get the inner WebSocket
    pub fn inner(&self) -> &WebSocket {
        &self.socket
    }
}

/// Handler for WebSocket-based wRPC connections in a Durable Object
pub struct WrpcWebSocketHandler {
    pair: WebSocketPair,
}

impl WrpcWebSocketHandler {
    /// Create a new WebSocket handler pair
    pub fn new() -> Result<Self> {
        let pair = WebSocketPair::new().map_err(|e| Error::Transport(e.to_string()))?;
        Ok(Self { pair })
    }

    /// Get the client WebSocket (to return in the HTTP response)
    pub fn client(&self) -> WrpcWebSocket {
        WrpcWebSocket::new(self.pair.client.clone())
    }

    /// Get the server WebSocket (to handle incoming messages)
    pub fn server(&self) -> WrpcWebSocket {
        WrpcWebSocket::new(self.pair.server.clone())
    }

    /// Accept the server-side connection and return the server socket
    pub fn accept(&self) -> Result<WrpcWebSocket> {
        let server = self.server();
        server.accept()?;
        Ok(server)
    }

    /// Get the client WebSocket for returning in Response::from_websocket
    pub fn into_client_websocket(self) -> WebSocket {
        self.pair.client
    }
}

impl Default for WrpcWebSocketHandler {
    fn default() -> Self {
        Self::new().expect("failed to create WebSocket pair")
    }
}

/// Parse a wRPC message from WebSocket text
pub fn parse_websocket_message(text: &str) -> Result<WrpcEnvelope> {
    serde_json::from_str(text).map_err(|e| Error::Protocol(format!("invalid message: {}", e)))
}

/// Extract wRPC request from envelope
pub fn extract_request(envelope: &WrpcEnvelope) -> Result<WrpcRequest> {
    use crate::message::WrpcMessageType;

    if envelope.msg_type != WrpcMessageType::Request {
        return Err(Error::Protocol("expected request message".to_string()));
    }

    serde_json::from_value(envelope.payload.clone())
        .map_err(|e| Error::Protocol(format!("invalid request: {}", e)))
}

/// Extract wRPC response from envelope
pub fn extract_response(envelope: &WrpcEnvelope) -> Result<WrpcResponse> {
    use crate::message::WrpcMessageType;

    if envelope.msg_type != WrpcMessageType::Response {
        return Err(Error::Protocol("expected response message".to_string()));
    }

    serde_json::from_value(envelope.payload.clone())
        .map_err(|e| Error::Protocol(format!("invalid response: {}", e)))
}
