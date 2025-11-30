//! WebSocket PingPong example demonstrating wRPC over WebSocket
//!
//! This example shows how to use WebSocket for wRPC communication with Durable Objects.
//!
//! Endpoints:
//! - GET /ws/:name - Connect to a WebSocket for DO "name"
//! - GET /health - Health check
//!
//! WebSocket Protocol:
//! - Send: {"type":"request","version":"0.1.0","payload":{"instance":"pingpong","function":"ping","params":{"message":"hello"},"version":"0.1.0"}}
//! - Recv: {"type":"response","version":"0.1.0","payload":{"status":"ok","data":{"message":"pong: hello","from":"name"},"version":"0.1.0"}}

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use worker::*;
use wrpc_transport_do::websocket::{extract_request, parse_websocket_message};
use wrpc_transport_do::{WrpcResponse, WrpcWebSocketHandler};

/// Ping request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingRequest {
    pub message: String,
}

/// Pong response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PongResponse {
    pub message: String,
    pub from: String,
}

/// WebSocket PingPong Durable Object
#[durable_object]
pub struct WebSocketPingPong {
    state: State,
    #[allow(dead_code)]
    env: Env,
}

impl DurableObject for WebSocketPingPong {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let path = req.path();

        // Set name
        if path.starts_with("/set-name/") {
            let name = path.trim_start_matches("/set-name/");
            self.state.storage().put("name", name).await?;
            return Response::ok("ok");
        }

        // Handle WebSocket upgrade request
        if path == "/ws" {
            return self.handle_websocket_upgrade().await;
        }

        // Health check
        if path == "/health" {
            return Response::ok("ok");
        }

        Response::error("Not found", 404)
    }
}

impl WebSocketPingPong {
    /// Get this DO's name from storage
    async fn get_name(&self) -> Result<String> {
        self.state
            .storage()
            .get("name")
            .await
            .map(|v: Option<String>| v.unwrap_or_else(|| "unknown".to_string()))
    }

    /// Handle WebSocket upgrade request
    async fn handle_websocket_upgrade(&self) -> Result<Response> {
        let handler = WrpcWebSocketHandler::new()
            .map_err(|e| Error::RustError(format!("failed to create WebSocket: {}", e)))?;

        let server = handler
            .accept()
            .map_err(|e| Error::RustError(format!("failed to accept WebSocket: {}", e)))?;

        // Get the name before spawning
        let name = self.get_name().await?;

        // Spawn a task to handle incoming messages
        wasm_bindgen_futures::spawn_local(async move {
            let mut event_stream = match server.inner().events() {
                Ok(stream) => stream,
                Err(_) => return,
            };

            while let Some(event) = event_stream.next().await {
                match event {
                    Ok(WebsocketEvent::Message(msg)) => {
                        if let Some(text) = msg.text() {
                            // Parse wRPC message
                            let response = match parse_websocket_message(&text) {
                                Ok(envelope) => match extract_request(&envelope) {
                                    Ok(request) => {
                                        // Handle the request
                                        Self::handle_request(&name, &request)
                                    }
                                    Err(e) => {
                                        WrpcResponse::error(format!("invalid request: {}", e))
                                    }
                                },
                                Err(e) => WrpcResponse::error(format!("parse error: {}", e)),
                            };

                            // Send response
                            if let Err(e) = server.send_response(&response) {
                                console_log!("failed to send response: {}", e);
                            }
                        }
                    }
                    Ok(WebsocketEvent::Close(_)) => {
                        break;
                    }
                    Err(e) => {
                        console_log!("WebSocket error: {:?}", e);
                        break;
                    }
                }
            }
        });

        // Return the client WebSocket
        Response::from_websocket(handler.into_client_websocket())
    }

    /// Handle a wRPC request (sync, no async needed)
    fn handle_request(name: &str, request: &wrpc_transport_do::WrpcRequest) -> WrpcResponse {
        if request.instance != "pingpong" {
            return WrpcResponse::not_found();
        }

        match request.function.as_str() {
            "ping" => {
                let ping: PingRequest = match request.decode_params() {
                    Ok(p) => p,
                    Err(e) => {
                        return WrpcResponse::error(format!("invalid params: {}", e));
                    }
                };

                let pong = PongResponse {
                    message: format!("pong: {}", ping.message),
                    from: name.to_string(),
                };

                WrpcResponse::ok(pong)
                    .unwrap_or_else(|e| WrpcResponse::error(format!("serialization error: {}", e)))
            }
            "echo" => {
                // Echo back the params as-is
                WrpcResponse::ok(request.params.clone())
                    .unwrap_or_else(|e| WrpcResponse::error(format!("serialization error: {}", e)))
            }
            _ => WrpcResponse::not_found(),
        }
    }
}

/// Worker entry point
#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let path = req.path();

    // GET /ws/:name - Connect to a WebSocket for DO "name"
    if path.starts_with("/ws/") {
        let name = path.trim_start_matches("/ws/");
        if name.is_empty() {
            return Response::error("Name required", 400);
        }

        let namespace = env.durable_object("WEBSOCKET_PINGPONG")?;
        let id = namespace.id_from_name(name)?;
        let stub = id.get_stub()?;

        // Set the DO's name first
        let set_name_url = format!("https://do/set-name/{}", name);
        stub.fetch_with_str(&set_name_url).await.ok();

        // Forward the WebSocket upgrade request
        let url = "https://do/ws";
        let ws_req = Request::new(url, Method::Get)?;
        return stub.fetch_with_request(ws_req).await;
    }

    // Health check
    if path == "/health" {
        return Response::ok("ok");
    }

    Response::error("Not found", 404)
}
