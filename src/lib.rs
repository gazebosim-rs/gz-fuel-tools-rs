// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Gazebo Fuel resource client, storage, and compatibility operations.
pub mod api;
pub mod cli;
mod client;
mod client_native;
mod command;
pub use api::{
    CachedResource, ClientConfig, Downloaded, FuelClient, ResourceId, ResourceIter, ResourceKind,
    ServerConfig,
};
mod core;
mod error;
mod ffi_pager;
mod identity;
mod io;
mod metadata;
mod pagination;
mod policy;
mod transport;
pub use error::{Error, ErrorKind};
pub use policy::ResourceLimits;

use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
};

/// Execute an internal compatibility operation.
pub(crate) fn execute(op: &str, args: &Value) -> Result<Value, String> {
    core::dispatch(op, args)
        .or_else(|| io::dispatch(op, args))
        .or_else(|| client::dispatch(op, args))
        .or_else(|| command::dispatch(op, args))
        .ok_or_else(|| format!("Unknown Fuel operation: {op}"))
}

#[doc(hidden)]
#[repr(C)]
pub struct FuelBuffer {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

fn respond(input: &[u8]) -> Vec<u8> {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<Value, String> {
        let request: Value = serde_json::from_slice(input).map_err(|e| e.to_string())?;
        let op = request["op"].as_str().ok_or("Missing operation")?;
        execute(op, &request["args"])
    }));
    let response = match result {
        Ok(Ok(value)) => json!({"value": value}),
        Ok(Err(error)) => json!({"error": error}),
        Err(_) => json!({"error":"Fuel operation panicked"}),
    };
    serde_json::to_vec(&response).expect("JSON response consists of valid JSON values")
}

/// # Safety
/// `input` must identify `len` readable bytes for the duration of this call.
/// A null pointer is accepted only with zero length. Free the returned buffer once
/// with `gz_fuel_rs_free`; do not alter its pointer, length or capacity.
// Keep test symbols distinct from the exported symbols in the CLI's copy of
// this library so LLVM can merge both binaries' coverage without name clashes.
#[cfg_attr(not(test), unsafe(no_mangle))]
#[doc(hidden)]
pub unsafe extern "C" fn gz_fuel_rs_call(input: *const u8, len: usize) -> FuelBuffer {
    if input.is_null() && len != 0 {
        return owned_buffer(br#"{"error":"null pointer with nonzero length"}"#.to_vec());
    }
    let bytes = if input.is_null() {
        &[]
    } else {
        // SAFETY: the caller guarantees a readable slice of the supplied length.
        unsafe { slice::from_raw_parts(input, len) }
    };
    owned_buffer(respond(bytes))
}

fn owned_buffer(mut bytes: Vec<u8>) -> FuelBuffer {
    let buffer = FuelBuffer {
        data: bytes.as_mut_ptr(),
        len: bytes.len(),
        capacity: bytes.capacity(),
    };
    std::mem::forget(bytes);
    buffer
}

/// Two separately owned buffers: JSON control metadata and uninterpreted body bytes.
#[doc(hidden)]
#[repr(C)]
pub struct FuelResponse {
    pub metadata: FuelBuffer,
    pub body: FuelBuffer,
}

fn respond_binary(input: &[u8], body: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(Value, Vec<u8>), String> {
        let request: Value = serde_json::from_slice(input).map_err(|e| e.to_string())?;
        let op = request["op"].as_str().ok_or("Missing operation")?;
        match op {
            "io.request" => Ok(transport::request_binary(&request["args"], body)),
            "io.cache_save" => Ok((io::cache_save_bytes(&request["args"], body), Vec::new())),
            _ => Err(format!("Unknown binary Fuel operation: {op}")),
        }
    }));
    let (metadata, body) = match result {
        Ok(Ok((value, body))) => (json!({"value": value}), body),
        Ok(Err(error)) => (json!({"error": error}), Vec::new()),
        Err(_) => (json!({"error": "Fuel operation panicked"}), Vec::new()),
    };
    (
        serde_json::to_vec(&metadata).expect("valid JSON response"),
        body,
    )
}

/// Execute a compatibility operation without encoding bodies as JSON arrays.
///
/// # Safety
/// Each non-null pointer identifies its stated number of readable bytes for this
/// synchronous call. A null pointer is valid only for an empty slice. Input memory
/// remains caller-owned; this function retains no borrowed pointers. Each returned
/// buffer must be passed unchanged to `gz_fuel_rs_free` exactly once. Calls share
/// only the synchronized HTTP client pool; output allocations are independent.
#[cfg_attr(not(test), unsafe(no_mangle))]
#[doc(hidden)]
pub unsafe extern "C" fn gz_fuel_rs_call_binary(
    input: *const u8,
    len: usize,
    body: *const u8,
    body_len: usize,
) -> FuelResponse {
    if (input.is_null() && len != 0) || (body.is_null() && body_len != 0) {
        return FuelResponse {
            metadata: owned_buffer(br#"{"error":"null pointer with nonzero length"}"#.to_vec()),
            body: owned_buffer(Vec::new()),
        };
    }
    // SAFETY: the caller guarantees readable ranges for both non-null pointers;
    // null/empty slices are handled without constructing a null Rust slice.
    let input = if input.is_null() {
        &[]
    } else {
        // SAFETY: the caller guarantees this non-null range is readable.
        unsafe { slice::from_raw_parts(input, len) }
    };
    // SAFETY: the same contract applies independently to the raw binary body.
    let body = if body.is_null() {
        &[]
    } else {
        // SAFETY: the caller independently guarantees the binary body range.
        unsafe { slice::from_raw_parts(body, body_len) }
    };
    let (metadata, body) = respond_binary(input, body);
    FuelResponse {
        metadata: owned_buffer(metadata),
        body: owned_buffer(body),
    }
}

/// # Safety
/// The buffer must be an unchanged result of `gz_fuel_rs_call`, not previously freed.
#[cfg_attr(not(test), unsafe(no_mangle))]
#[doc(hidden)]
pub unsafe extern "C" fn gz_fuel_rs_free(buffer: FuelBuffer) {
    if !buffer.data.is_null() {
        // SAFETY: buffer retains the allocation and metadata returned by the call.
        drop(unsafe { Vec::from_raw_parts(buffer.data, buffer.len, buffer.capacity) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_boundary_keeps_bodies_out_of_json_and_frees_both_buffers() {
        for input in [b"{".as_slice(), b"{}", br#"{"op":"missing"}"#] {
            let (metadata, body) = respond_binary(input, &[0, 255, 0]);
            assert!(serde_json::from_slice::<Value>(&metadata).unwrap()["error"].is_string());
            assert!(body.is_empty());
        }
        let input = br#"{"op":"io.cache_save","args":{"kind":"invalid"}}"#;
        // SAFETY: all input buffers are valid for this call. Both returned buffers
        // are freed once, including the empty body allocation.
        unsafe {
            let response =
                gz_fuel_rs_call_binary(input.as_ptr(), input.len(), [0, 255].as_ptr(), 2);
            let metadata: Value = serde_json::from_slice(slice::from_raw_parts(
                response.metadata.data,
                response.metadata.len,
            ))
            .unwrap();
            assert_eq!(metadata["value"]["ok"], false);
            assert!(metadata["value"].get("data").is_none());
            gz_fuel_rs_free(response.metadata);
            gz_fuel_rs_free(response.body);
            let response = gz_fuel_rs_call_binary(std::ptr::null(), 1, std::ptr::null(), 0);
            gz_fuel_rs_free(response.metadata);
            gz_fuel_rs_free(response.body);
            let response = gz_fuel_rs_call_binary(std::ptr::null(), 0, std::ptr::null(), 0);
            gz_fuel_rs_free(response.metadata);
            gz_fuel_rs_free(response.body);
        }
    }
    #[test]
    fn boundary_rejects_invalid_requests_and_preserves_unicode() {
        for request in [b"{".as_slice(), b"{}", br#"{"op":"missing"}"#] {
            let output: Value = serde_json::from_slice(&respond(request)).unwrap();
            assert!(output["error"].is_string());
        }
        let input = br#"{"op":"sanitize_authority","args":{"authority":"a:b"}}"#;
        // SAFETY: input is live throughout this call and the output is freed exactly once.
        unsafe {
            let output = gz_fuel_rs_call(input.as_ptr(), input.len());
            assert!(output.len > 0);
            gz_fuel_rs_free(output);
            let invalid = gz_fuel_rs_call(std::ptr::null(), 1);
            let metadata: Value =
                serde_json::from_slice(slice::from_raw_parts(invalid.data, invalid.len)).unwrap();
            assert!(metadata["error"].is_string());
            gz_fuel_rs_free(invalid);
            let empty = gz_fuel_rs_call(std::ptr::null(), 0);
            gz_fuel_rs_free(empty);
            gz_fuel_rs_free(FuelBuffer {
                data: std::ptr::null_mut(),
                len: 0,
                capacity: 0,
            });
        }
    }
}
