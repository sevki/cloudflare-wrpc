//! wRPC message types for Durable Object communication

use bytes::Bytes;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{Error, Result};

/// wRPC protocol version
pub const WRPC_VERSION: &str = "0.1.0";

/// Content type for wRPC messages
pub const WRPC_CONTENT_TYPE: &str = "application/x-wrpc+json";

/// HTTP header for wRPC instance
pub const WRPC_INSTANCE_HEADER: &str = "X-Wrpc-Instance";

/// HTTP header for wRPC function
pub const WRPC_FUNCTION_HEADER: &str = "X-Wrpc-Function";

/// HTTP header for wRPC version
pub const WRPC_VERSION_HEADER: &str = "X-Wrpc-Version";

/// A wRPC request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrpcRequest {
    /// The WIT instance being invoked
    pub instance: String,
    /// The function being called
    pub function: String,
    /// Serialized parameters (JSON encoded)
    pub params: Bytes,
    /// Protocol version
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    WRPC_VERSION.to_string()
}

impl WrpcRequest {
    /// Create a new wRPC request
    pub fn new<P: Serialize>(instance: &str, function: &str, params: P) -> Result<Self> {
        let params_json = serde_json::to_vec(&params)?;
        Ok(Self {
            instance: instance.to_string(),
            function: function.to_string(),
            params: Bytes::from(params_json),
            version: WRPC_VERSION.to_string(),
        })
    }

    /// Decode the parameters from the request
    pub fn decode_params<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.params).map_err(|e| Error::Serialization(e.to_string()))
    }

    /// Create a request from HTTP headers and body
    pub fn from_http(
        instance: &str,
        function: &str,
        body: Bytes,
    ) -> Self {
        Self {
            instance: instance.to_string(),
            function: function.to_string(),
            params: body,
            version: WRPC_VERSION.to_string(),
        }
    }

    /// Get the URL path for this request
    pub fn path(&self) -> String {
        format!("/_wrpc/{}/{}", self.instance, self.function)
    }
}

/// Status of a wRPC response
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WrpcStatus {
    /// Success
    Ok,
    /// Error
    Error,
    /// Not found
    NotFound,
}

/// A wRPC response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrpcResponse {
    /// Response status
    pub status: WrpcStatus,
    /// Serialized result (JSON encoded) or error message
    pub data: Bytes,
    /// Protocol version
    #[serde(default = "default_version")]
    pub version: String,
}

impl WrpcResponse {
    /// Create a successful response with the given result
    pub fn ok<T: Serialize>(result: T) -> Result<Self> {
        let data = serde_json::to_vec(&result)?;
        Ok(Self {
            status: WrpcStatus::Ok,
            data: Bytes::from(data),
            version: WRPC_VERSION.to_string(),
        })
    }

    /// Create an error response
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            status: WrpcStatus::Error,
            data: Bytes::from(message.into()),
            version: WRPC_VERSION.to_string(),
        }
    }

    /// Create a not found response
    pub fn not_found() -> Self {
        Self {
            status: WrpcStatus::NotFound,
            data: Bytes::from("function not found"),
            version: WRPC_VERSION.to_string(),
        }
    }

    /// Decode the response data
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        match self.status {
            WrpcStatus::Ok => {
                serde_json::from_slice(&self.data).map_err(|e| Error::Serialization(e.to_string()))
            }
            WrpcStatus::Error => {
                Err(Error::Protocol(String::from_utf8_lossy(&self.data).to_string()))
            }
            WrpcStatus::NotFound => Err(Error::NotFound {
                instance: String::new(),
                function: String::new(),
            }),
        }
    }

    /// Check if the response is successful
    pub fn is_ok(&self) -> bool {
        self.status == WrpcStatus::Ok
    }

    /// Check if the response is an error
    pub fn is_error(&self) -> bool {
        self.status == WrpcStatus::Error
    }

    /// Convert to HTTP status code
    pub fn http_status(&self) -> u16 {
        match self.status {
            WrpcStatus::Ok => 200,
            WrpcStatus::Error => 500,
            WrpcStatus::NotFound => 404,
        }
    }
}

/// Envelope for wRPC messages over HTTP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrpcEnvelope {
    /// Protocol version
    pub version: String,
    /// Message type
    #[serde(rename = "type")]
    pub msg_type: WrpcMessageType,
    /// Payload
    pub payload: serde_json::Value,
}

/// Type of wRPC message
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WrpcMessageType {
    /// Request message
    Request,
    /// Response message
    Response,
}

impl WrpcEnvelope {
    /// Create a request envelope
    pub fn request(request: &WrpcRequest) -> Result<Self> {
        Ok(Self {
            version: WRPC_VERSION.to_string(),
            msg_type: WrpcMessageType::Request,
            payload: serde_json::to_value(request)?,
        })
    }

    /// Create a response envelope
    pub fn response(response: &WrpcResponse) -> Result<Self> {
        Ok(Self {
            version: WRPC_VERSION.to_string(),
            msg_type: WrpcMessageType::Response,
            payload: serde_json::to_value(response)?,
        })
    }
}
