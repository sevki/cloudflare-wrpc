//! wRPC handler for Durable Objects

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use serde::{de::DeserializeOwned, Serialize};
use worker::{Request, Response};

use crate::message::{
    WrpcEnvelope, WrpcMessageType, WrpcRequest, WrpcResponse, WRPC_CONTENT_TYPE,
    WRPC_FUNCTION_HEADER, WRPC_INSTANCE_HEADER,
};
use crate::{Error, Result};

/// Trait for handling wRPC requests in a Durable Object
pub trait WrpcHandler {
    /// Handle an incoming wRPC request
    fn handle_wrpc(
        &self,
        request: WrpcRequest,
    ) -> impl Future<Output = Result<WrpcResponse>> + Send;
}

/// Type alias for handler functions
pub type HandlerFn =
    Box<dyn Fn(Bytes) -> Pin<Box<dyn Future<Output = Result<WrpcResponse>> + Send>> + Send + Sync>;

/// Router for dispatching wRPC requests to handlers
pub struct WrpcRouter {
    handlers: HashMap<(String, String), HandlerFn>,
}

impl Default for WrpcRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl WrpcRouter {
    /// Create a new router
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a handler for a specific instance/function pair
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut router = WrpcRouter::new();
    /// router.register("my-service", "greet", |name: String| async move {
    ///     Ok(format!("Hello, {}!", name))
    /// });
    /// ```
    pub fn register<P, R, F, Fut>(&mut self, instance: &str, function: &str, handler: F)
    where
        P: DeserializeOwned + Send + 'static,
        R: Serialize + Send + 'static,
        F: Fn(P) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
    {
        let handler = move |params: Bytes| {
            let params: std::result::Result<P, _> = serde_json::from_slice(&params);
            let fut =
                match params {
                    Ok(p) => {
                        let fut = handler(p);
                        Box::pin(async move {
                            let result = fut.await?;
                            WrpcResponse::ok(result)
                        })
                            as Pin<Box<dyn Future<Output = Result<WrpcResponse>> + Send>>
                    }
                    Err(e) => Box::pin(async move {
                        Ok(WrpcResponse::error(format!("invalid params: {}", e)))
                    })
                        as Pin<Box<dyn Future<Output = Result<WrpcResponse>> + Send>>,
                };
            fut
        };

        self.handlers.insert(
            (instance.to_string(), function.to_string()),
            Box::new(handler),
        );
    }

    /// Handle a wRPC request using the registered handlers
    pub async fn handle(&self, request: WrpcRequest) -> Result<WrpcResponse> {
        let key = (request.instance.clone(), request.function.clone());

        match self.handlers.get(&key) {
            Some(handler) => handler(request.params).await,
            None => Ok(WrpcResponse::not_found()),
        }
    }

    /// Handle an HTTP request and return an HTTP response
    ///
    /// This is the main entry point for integrating with Durable Object's fetch handler
    pub async fn handle_http(&self, req: Request) -> worker::Result<Response> {
        // Check if this is a wRPC request
        if !is_wrpc_request(&req) {
            return Response::error("Not a wRPC request", 400);
        }

        // Parse the wRPC request
        let wrpc_request = match parse_wrpc_request(req).await {
            Ok(r) => r,
            Err(e) => {
                let response = WrpcResponse::error(format!("failed to parse request: {}", e));
                return wrpc_response_to_http(response);
            }
        };

        // Handle the request
        let wrpc_response = match self.handle(wrpc_request).await {
            Ok(r) => r,
            Err(e) => WrpcResponse::error(format!("handler error: {}", e)),
        };

        wrpc_response_to_http(wrpc_response)
    }
}

/// Check if an HTTP request is a wRPC request
pub fn is_wrpc_request(req: &Request) -> bool {
    // Check by path prefix
    if req.path().starts_with("/_wrpc/") {
        return true;
    }

    // Check by content type
    if let Ok(Some(ct)) = req.headers().get("Content-Type") {
        if ct.contains("application/x-wrpc") {
            return true;
        }
    }

    // Check by wRPC headers
    if req.headers().get(WRPC_INSTANCE_HEADER).is_ok() {
        return true;
    }

    false
}

/// Parse a wRPC request from an HTTP request
pub async fn parse_wrpc_request(mut req: Request) -> Result<WrpcRequest> {
    let path = req.path();

    // Try to extract instance and function from path (/_wrpc/{instance}/{function})
    let (instance, function) = if path.starts_with("/_wrpc/") {
        let parts: Vec<&str> = path.trim_start_matches("/_wrpc/").split('/').collect();
        if parts.len() >= 2 {
            (parts[0].to_string(), parts[1].to_string())
        } else {
            // Try headers
            extract_from_headers(&req)?
        }
    } else {
        extract_from_headers(&req)?
    };

    // Get the body
    let body = req
        .text()
        .await
        .map_err(|e| Error::Transport(format!("failed to read body: {}", e)))?;

    // Try to parse as envelope first
    if let Ok(envelope) = serde_json::from_str::<WrpcEnvelope>(&body) {
        if envelope.msg_type == WrpcMessageType::Request {
            let request: WrpcRequest = serde_json::from_value(envelope.payload)
                .map_err(|e| Error::Protocol(format!("invalid request payload: {}", e)))?;
            return Ok(request);
        }
    }

    // Otherwise, treat body as params directly
    Ok(WrpcRequest::from_http(
        &instance,
        &function,
        Bytes::from(body),
    ))
}

fn extract_from_headers(req: &Request) -> Result<(String, String)> {
    let headers = req.headers();

    let instance = headers
        .get(WRPC_INSTANCE_HEADER)
        .map_err(|e| Error::Protocol(format!("failed to get instance header: {}", e)))?
        .ok_or_else(|| Error::Protocol("missing instance header".to_string()))?;

    let function = headers
        .get(WRPC_FUNCTION_HEADER)
        .map_err(|e| Error::Protocol(format!("failed to get function header: {}", e)))?
        .ok_or_else(|| Error::Protocol("missing function header".to_string()))?;

    Ok((instance, function))
}

/// Convert a wRPC response to an HTTP response
pub fn wrpc_response_to_http(response: WrpcResponse) -> worker::Result<Response> {
    let envelope = WrpcEnvelope::response(&response)
        .map_err(|e| worker::Error::RustError(format!("failed to create envelope: {}", e)))?;

    let body = serde_json::to_string(&envelope)
        .map_err(|e| worker::Error::RustError(format!("failed to serialize response: {}", e)))?;

    let headers = worker::Headers::new();
    headers.set("Content-Type", WRPC_CONTENT_TYPE)?;

    Response::ok(body).map(|r| r.with_headers(headers))
}

/// Helper macro to create a router with handlers
///
/// # Example
///
/// ```rust,ignore
/// let router = wrpc_router! {
///     "my-service" => {
///         "greet" => |name: String| async move {
///             Ok(format!("Hello, {}!", name))
///         },
///         "add" => |params: (i32, i32)| async move {
///             Ok(params.0 + params.1)
///         },
///     }
/// };
/// ```
#[macro_export]
macro_rules! wrpc_router {
    (
        $($instance:literal => {
            $($function:literal => $handler:expr),* $(,)?
        }),* $(,)?
    ) => {{
        let mut router = $crate::WrpcRouter::new();
        $(
            $(
                router.register($instance, $function, $handler);
            )*
        )*
        router
    }};
}
