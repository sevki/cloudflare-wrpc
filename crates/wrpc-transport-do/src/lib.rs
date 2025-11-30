//! wRPC transport for Cloudflare Durable Objects
//!
//! This crate provides a transport layer that enables Durable Objects to communicate
//! with each other using wRPC semantics over HTTP fetch requests.
//!
//! # Example
//!
//! ```rust,ignore
//! use wrpc_transport_do::{DurableObjectClient, WrpcHandler};
//!
//! // In your DO, implement the handler
//! impl WrpcHandler for MyDurableObject {
//!     async fn handle_wrpc(&self, request: WrpcRequest) -> Result<WrpcResponse> {
//!         match (request.instance.as_str(), request.function.as_str()) {
//!             ("my-service", "greet") => {
//!                 let name: String = request.decode_params()?;
//!                 Ok(WrpcResponse::ok(format!("Hello, {}!", name)))
//!             }
//!             _ => Ok(WrpcResponse::not_found()),
//!         }
//!     }
//! }
//!
//! // To call another DO
//! let client = DurableObjectClient::new(stub);
//! let result: String = client.invoke("my-service", "greet", "World").await?;
//! ```

mod client;
mod error;
mod handler;
pub mod message;
pub mod websocket;

pub use client::{ClientBuilder, DurableObjectClient};
pub use error::{Error, Result};
pub use handler::{is_wrpc_request, WrpcHandler, WrpcRouter};
pub use message::{WrpcEnvelope, WrpcRequest, WrpcResponse};
pub use websocket::{WrpcWebSocket, WrpcWebSocketHandler};
