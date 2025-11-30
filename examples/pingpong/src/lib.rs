//! PingPong Durable Object example demonstrating wRPC DO-to-DO communication
//!
//! Flow:
//! 1. Worker receives GET /ping/:from/:to
//! 2. Worker instantiates DO "from" and fetches to it
//! 3. DO "from" uses wRPC to call DO "to" (different instance)
//! 4. DO "to" responds with pong via wRPC

use serde::{Deserialize, Serialize};
use worker::*;
use wrpc_transport_do::{ClientBuilder, WrpcResponse};

/// Ping message sent via wRPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingMessage {
    pub from: String,
    pub message: String,
}

/// Pong response via wRPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PongResponse {
    pub from: String,
    pub message: String,
    pub received_ping: PingMessage,
}

/// PingPong Durable Object
#[durable_object]
pub struct PingPong {
    state: State,
    env: Env,
}

impl DurableObject for PingPong {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let path = req.path();

        // Handle wRPC requests (from other DOs)
        if path.starts_with("/_wrpc/") {
            return self.handle_wrpc_request(req).await;
        }

        // Handle regular fetch from worker: /send-ping/:target
        if path.starts_with("/send-ping/") {
            let target = path.trim_start_matches("/send-ping/");
            return self.send_ping_to(target).await;
        }

        Response::error("Not found", 404)
    }
}

impl PingPong {
    /// Get this DO's name from storage
    async fn get_name(&self) -> Result<String> {
        self.state
            .storage()
            .get("name")
            .await
            .map(|v: Option<String>| v.unwrap_or_else(|| "unknown".to_string()))
    }

    /// Set this DO's name
    async fn set_name(&self, name: &str) -> Result<()> {
        self.state.storage().put("name", name).await
    }

    /// Send ping to another DO instance via wRPC
    async fn send_ping_to(&self, target: &str) -> Result<Response> {
        let my_name = self.get_name().await?;

        // Get the target DO namespace and create wRPC client
        let namespace = self.env.durable_object("PINGPONG")?;
        let client = ClientBuilder::new(namespace).by_name(target)?;

        // Send ping via wRPC
        let ping = PingMessage {
            from: my_name.clone(),
            message: format!("Hello from {}!", my_name),
        };

        let pong: PongResponse = client
            .invoke("pingpong", "ping", ping)
            .await
            .map_err(|e| Error::RustError(format!("wRPC call failed: {}", e)))?;

        Response::from_json(&pong)
    }

    /// Handle incoming wRPC request
    async fn handle_wrpc_request(&self, mut req: Request) -> Result<Response> {
        let path = req.path();
        let parts: Vec<&str> = path.trim_start_matches("/_wrpc/").split('/').collect();

        if parts.len() < 2 {
            return Response::error("Invalid wRPC path", 400);
        }

        let instance = parts[0];
        let function = parts[1];

        if instance != "pingpong" {
            return Response::error("Unknown instance", 404);
        }

        let body = req.text().await?;

        let response = match function {
            "ping" => {
                let ping: PingMessage = self.parse_wrpc_params(&body)?;
                let my_name = self.get_name().await?;

                let pong = PongResponse {
                    from: my_name,
                    message: "pong!".to_string(),
                    received_ping: ping,
                };

                WrpcResponse::ok(pong)?
            }
            "set_name" => {
                let name: String = self.parse_wrpc_params(&body)?;
                self.set_name(&name).await?;
                WrpcResponse::ok(())?
            }
            _ => WrpcResponse::not_found(),
        };

        // Return wRPC response
        let envelope = wrpc_transport_do::message::WrpcEnvelope::response(&response)
            .map_err(|e| Error::RustError(e.to_string()))?;
        let body = serde_json::to_string(&envelope)?;

        let headers = Headers::new();
        headers.set("Content-Type", "application/x-wrpc+json")?;

        Response::ok(body).map(|r| r.with_headers(headers))
    }

    /// Parse params from wRPC request body
    fn parse_wrpc_params<T: serde::de::DeserializeOwned>(&self, body: &str) -> Result<T> {
        let envelope: serde_json::Value = serde_json::from_str(body)
            .map_err(|e| Error::RustError(format!("invalid JSON: {}", e)))?;

        if let Some(payload) = envelope.get("payload") {
            if let Some(params) = payload.get("params") {
                return serde_json::from_value(params.clone())
                    .map_err(|e| Error::RustError(format!("invalid params: {}", e)));
            }
        }

        Err(Error::RustError("missing params".to_string()))
    }
}

/// Worker entry point
#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let path = req.path();

    // GET /ping/:from/:to - Have DO "from" ping DO "to" via wRPC
    if path.starts_with("/ping/") {
        let parts: Vec<&str> = path.trim_start_matches("/ping/").split('/').collect();
        if parts.len() != 2 {
            return Response::error("Usage: /ping/:from/:to", 400);
        }

        let from_name = parts[0];
        let to_name = parts[1];

        let namespace = env.durable_object("PINGPONG")?;

        // First, set both DOs' names via wRPC
        let from_client = ClientBuilder::new(namespace.clone()).by_name(from_name)?;
        from_client
            .invoke::<_, ()>("pingpong", "set_name", from_name.to_string())
            .await
            .map_err(|e| Error::RustError(format!("set_name failed: {}", e)))?;

        let to_client = ClientBuilder::new(namespace.clone()).by_name(to_name)?;
        to_client
            .invoke::<_, ()>("pingpong", "set_name", to_name.to_string())
            .await
            .map_err(|e| Error::RustError(format!("set_name failed: {}", e)))?;

        // Instantiate the "from" DO and fetch to it
        let from_id = namespace.id_from_name(from_name)?;
        let from_stub = from_id.get_stub()?;

        // Fetch to the DO with the target name
        let url = format!("https://do/send-ping/{}", to_name);
        let do_req = Request::new(&url, Method::Get)?;

        // The DO will use wRPC to call the target
        return from_stub.fetch_with_request(do_req).await;
    }

    // Health check
    if path == "/health" {
        return Response::ok("ok");
    }

    Response::error("Not found", 404)
}
