//! Example Cloudflare Worker demonstrating wRPC communication between Durable Objects
//!
//! This worker implements two Durable Objects:
//! - `CounterService`: A service that maintains a counter and exposes wRPC methods
//! - `OrchestratorService`: A service that calls the CounterService via wRPC
//!
//! The worker exposes HTTP endpoints to test the communication:
//! - GET /counter/{name}/value - Get current counter value
//! - POST /counter/{name}/increment - Increment counter
//! - POST /orchestrate - Orchestrate multiple counter operations via wRPC

use serde::{Deserialize, Serialize};
use worker::*;
use wrpc_transport_do::{is_wrpc_request, ClientBuilder, WrpcResponse};

/// Counter service - maintains a counter and exposes wRPC methods
#[durable_object]
pub struct CounterService {
    state: State,
    #[allow(dead_code)]
    env: Env,
}

impl DurableObject for CounterService {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        // Check if this is a wRPC request
        if is_wrpc_request(&req) {
            return self.handle_wrpc(req).await;
        }

        // Handle regular HTTP requests
        let path = req.path();
        let method = req.method();

        match (method, path.as_str()) {
            (Method::Get, "/value") => {
                let value = self.get_counter().await?;
                Response::ok(value.to_string())
            }
            (Method::Post, "/increment") => {
                let value = self.increment(1).await?;
                Response::ok(value.to_string())
            }
            (Method::Post, "/reset") => {
                self.reset().await?;
                Response::ok("reset")
            }
            _ => Response::error("Not Found", 404),
        }
    }
}

impl CounterService {
    async fn get_counter(&self) -> Result<i64> {
        self.state
            .storage()
            .get("counter")
            .await
            .map(|v: Option<i64>| v.unwrap_or(0))
    }

    async fn increment(&self, amount: i64) -> Result<i64> {
        let current = self.get_counter().await?;
        let new_value = current + amount;
        self.state.storage().put("counter", new_value).await?;
        Ok(new_value)
    }

    async fn reset(&self) -> Result<()> {
        self.state.storage().put("counter", 0i64).await?;
        Ok(())
    }

    async fn handle_wrpc(&self, mut req: Request) -> Result<Response> {
        let path = req.path();

        // Parse the wRPC path: /_wrpc/{instance}/{function}
        let parts: Vec<&str> = path.trim_start_matches("/_wrpc/").split('/').collect();

        if parts.len() < 2 {
            return Response::error("Invalid wRPC path", 400);
        }

        let instance = parts[0];
        let function = parts[1];

        // Handle counter-specific wRPC calls
        if instance == "counter" {
            let body = req.text().await?;

            let response = match function {
                "get" => {
                    let value = self.get_counter().await?;
                    WrpcResponse::ok(value)?
                }
                "increment" => {
                    // Parse the amount from the request body
                    let envelope: serde_json::Value = serde_json::from_str(&body)
                        .map_err(|e| Error::RustError(format!("invalid JSON: {}", e)))?;

                    let amount: i64 = if let Some(payload) = envelope.get("payload") {
                        if let Some(params) = payload.get("params") {
                            serde_json::from_value(params.clone()).unwrap_or(1)
                        } else {
                            1
                        }
                    } else {
                        serde_json::from_str(&body).unwrap_or(1)
                    };

                    let value = self.increment(amount).await?;
                    WrpcResponse::ok(value)?
                }
                "reset" => {
                    self.reset().await?;
                    WrpcResponse::ok(())?
                }
                _ => WrpcResponse::not_found(),
            };

            // Return the response as JSON
            let envelope = wrpc_transport_do::message::WrpcEnvelope::response(&response)
                .map_err(|e| Error::RustError(e.to_string()))?;
            let body = serde_json::to_string(&envelope)?;

            let headers = Headers::new();
            headers.set("Content-Type", "application/x-wrpc+json")?;

            Response::ok(body).map(|r| r.with_headers(headers))
        } else {
            Response::error("Unknown instance", 404)
        }
    }
}

/// Orchestrator service - coordinates calls to multiple counter services via wRPC
#[durable_object]
pub struct OrchestratorService {
    #[allow(dead_code)]
    state: State,
    env: Env,
}

/// Parameters for the orchestrate operation
#[derive(Debug, Serialize, Deserialize)]
pub struct OrchestrateParams {
    pub counters: Vec<String>,
    pub operation: String,
    pub amount: Option<i64>,
}

/// Result of the orchestrate operation
#[derive(Debug, Serialize, Deserialize)]
pub struct OrchestrateResult {
    pub results: Vec<CounterResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CounterResult {
    pub name: String,
    pub value: i64,
}

impl DurableObject for OrchestratorService {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let path = req.path();
        let method = req.method();

        match (method, path.as_str()) {
            (Method::Post, "/orchestrate") => {
                let params: OrchestrateParams = req.json().await?;
                let result = self.orchestrate(params).await?;
                Response::from_json(&result)
            }
            (Method::Get, "/health") => Response::ok("healthy"),
            _ => Response::error("Not Found", 404),
        }
    }
}

impl OrchestratorService {
    async fn orchestrate(&self, params: OrchestrateParams) -> Result<OrchestrateResult> {
        let counter_namespace = self.env.durable_object("COUNTER_SERVICE")?;
        let builder = ClientBuilder::new(counter_namespace);

        let mut results = Vec::new();

        for counter_name in &params.counters {
            let client = builder.by_name(counter_name)?;

            let value: i64 = match params.operation.as_str() {
                "get" => client
                    .invoke("counter", "get", ())
                    .await
                    .map_err(|e| Error::RustError(e.to_string()))?,
                "increment" => {
                    let amount = params.amount.unwrap_or(1);
                    client
                        .invoke("counter", "increment", amount)
                        .await
                        .map_err(|e| Error::RustError(e.to_string()))?
                }
                "reset" => {
                    client
                        .invoke::<_, ()>("counter", "reset", ())
                        .await
                        .map_err(|e| Error::RustError(e.to_string()))?;
                    0
                }
                _ => {
                    return Err(Error::RustError(format!(
                        "unknown operation: {}",
                        params.operation
                    )))
                }
            };

            results.push(CounterResult {
                name: counter_name.clone(),
                value,
            });
        }

        Ok(OrchestrateResult { results })
    }
}

/// Main worker entry point
#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let router = Router::new();

    router
        // Direct counter access
        .get_async("/counter/:name/value", |_req, ctx| async move {
            let name = ctx.param("name").unwrap();
            let namespace = ctx.env.durable_object("COUNTER_SERVICE")?;
            let stub = namespace.id_from_name(name)?.get_stub()?;
            stub.fetch_with_str("https://do.internal/value").await
        })
        .post_async("/counter/:name/increment", |_req, ctx| async move {
            let name = ctx.param("name").unwrap();
            let namespace = ctx.env.durable_object("COUNTER_SERVICE")?;
            let stub = namespace.id_from_name(name)?.get_stub()?;
            stub.fetch_with_str("https://do.internal/increment").await
        })
        .post_async("/counter/:name/reset", |_req, ctx| async move {
            let name = ctx.param("name").unwrap();
            let namespace = ctx.env.durable_object("COUNTER_SERVICE")?;
            let stub = namespace.id_from_name(name)?.get_stub()?;
            stub.fetch_with_str("https://do.internal/reset").await
        })
        // Orchestrated operations via wRPC
        .post_async("/orchestrate", |req, ctx| async move {
            let namespace = ctx.env.durable_object("ORCHESTRATOR_SERVICE")?;
            let stub = namespace.id_from_name("orchestrator")?.get_stub()?;

            // Forward the request to the orchestrator DO
            stub.fetch_with_request(req).await
        })
        // Health check
        .get("/health", |_req, _ctx| Response::ok("ok"))
        .run(req, env)
        .await
}
