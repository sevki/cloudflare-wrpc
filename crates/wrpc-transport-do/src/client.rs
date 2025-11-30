//! wRPC client for invoking functions on Durable Objects

use serde::{de::DeserializeOwned, Serialize};
use wasm_bindgen::JsValue;
use worker::durable::Stub;
use worker::{Headers, Method, Request, RequestInit};

use crate::message::{
    WrpcEnvelope, WrpcRequest, WrpcResponse, WRPC_CONTENT_TYPE, WRPC_FUNCTION_HEADER,
    WRPC_INSTANCE_HEADER, WRPC_VERSION, WRPC_VERSION_HEADER,
};
use crate::{Error, Result};

/// Client for making wRPC calls to a Durable Object
pub struct DurableObjectClient {
    stub: Stub,
}

impl DurableObjectClient {
    /// Create a new client for the given Durable Object stub
    pub fn new(stub: Stub) -> Self {
        Self { stub }
    }

    /// Invoke a function on the target Durable Object
    ///
    /// # Arguments
    ///
    /// * `instance` - The WIT instance name (e.g., "my-service")
    /// * `function` - The function name (e.g., "greet")
    /// * `params` - The function parameters (will be serialized to JSON)
    ///
    /// # Returns
    ///
    /// The deserialized result from the function call
    pub async fn invoke<P, R>(&self, instance: &str, function: &str, params: P) -> Result<R>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let request = WrpcRequest::new(instance, function, params)?;
        let response = self.invoke_raw(request).await?;
        response.decode()
    }

    /// Invoke a function and return the raw response
    pub async fn invoke_raw(&self, request: WrpcRequest) -> Result<WrpcResponse> {
        let envelope = WrpcEnvelope::request(&request)?;
        let body = serde_json::to_string(&envelope)?;

        // Build the HTTP request headers
        let headers = Headers::new();
        headers
            .set("Content-Type", WRPC_CONTENT_TYPE)
            .map_err(|e| Error::Transport(e.to_string()))?;
        headers
            .set(WRPC_VERSION_HEADER, WRPC_VERSION)
            .map_err(|e| Error::Transport(e.to_string()))?;
        headers
            .set(WRPC_INSTANCE_HEADER, &request.instance)
            .map_err(|e| Error::Transport(e.to_string()))?;
        headers
            .set(WRPC_FUNCTION_HEADER, &request.function)
            .map_err(|e| Error::Transport(e.to_string()))?;

        // Build RequestInit with body
        let mut init = RequestInit::new();
        init.with_method(Method::Post);
        init.with_headers(headers);
        init.with_body(Some(JsValue::from_str(&body)));

        // Create the request URL (using the wRPC path format)
        let url = format!("https://do.internal{}", request.path());

        let req =
            Request::new_with_init(&url, &init).map_err(|e| Error::Transport(e.to_string()))?;

        // Send to the Durable Object
        let mut response = self.stub.fetch_with_request(req).await?;

        // Parse the response
        let response_body = response
            .text()
            .await
            .map_err(|e| Error::Transport(format!("failed to read response body: {}", e)))?;

        let envelope: WrpcEnvelope = serde_json::from_str(&response_body)
            .map_err(|e| Error::Protocol(format!("invalid response envelope: {}", e)))?;

        let wrpc_response: WrpcResponse = serde_json::from_value(envelope.payload)
            .map_err(|e| Error::Protocol(format!("invalid response payload: {}", e)))?;

        Ok(wrpc_response)
    }
}

/// Builder for creating Durable Object clients
pub struct ClientBuilder {
    namespace: worker::durable::ObjectNamespace,
}

impl ClientBuilder {
    /// Create a new client builder from a Durable Object namespace
    pub fn new(namespace: worker::durable::ObjectNamespace) -> Self {
        Self { namespace }
    }

    /// Get a client for a Durable Object by name
    pub fn by_name(&self, name: &str) -> Result<DurableObjectClient> {
        let stub = self.namespace.get_by_name(name)?;
        Ok(DurableObjectClient::new(stub))
    }

    /// Get a client for a Durable Object by ID string
    pub fn by_id(&self, id: &str) -> Result<DurableObjectClient> {
        let object_id = self.namespace.id_from_string(id)?;
        let stub = object_id.get_stub()?;
        Ok(DurableObjectClient::new(stub))
    }

    /// Get a client for a unique Durable Object
    pub fn unique(&self) -> Result<DurableObjectClient> {
        let object_id = self.namespace.unique_id()?;
        let stub = object_id.get_stub()?;
        Ok(DurableObjectClient::new(stub))
    }
}
