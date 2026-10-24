// SPDX-License-Identifier: Apache-2.0
//! Gazebo Fuel resource client, storage, and compatibility operations.
pub mod api;
pub mod cli;
mod client;
mod command;
pub use api::{
    CachedResource, ClientConfig, Downloaded, Error, FuelClient, ResourceId, ResourceIter,
    ResourceKind, ServerConfig,
};
mod core;
mod io;

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
    let bytes = if input.is_null() {
        &[]
    } else {
        // SAFETY: the caller guarantees a readable slice of the supplied length.
        unsafe { slice::from_raw_parts(input, len) }
    };
    let mut output = respond(bytes);
    let buffer = FuelBuffer {
        data: output.as_mut_ptr(),
        len: output.len(),
        capacity: output.capacity(),
    };
    std::mem::forget(output);
    buffer
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
