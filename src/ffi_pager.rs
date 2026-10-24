// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Opaque, uniquely owned pagination state across the C++ boundary.
use crate::{ClientConfig, Error, FuelBuffer, ResourceKind, ServerConfig, pagination::Pager};
use serde::Deserialize;
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
};
#[derive(Deserialize)]
struct Arguments {
    server: ServerConfig,
    path: String,
    kind: String,
    #[serde(default)]
    user_agent: String,
}
struct Handle {
    pager: Option<Pager>,
    error: Option<String>,
}
fn create(input: &[u8]) -> Handle {
    let result = (|| -> Result<Pager, Error> {
        let args: Arguments = serde_json::from_slice(input)?;
        let kind = match args.kind.as_str() {
            "models" => ResourceKind::Model,
            "worlds" => ResourceKind::World,
            _ => {
                return Err(Error::new(
                    crate::ErrorKind::InvalidInput,
                    "Invalid listing kind",
                ));
            }
        };
        let config = ClientConfig {
            user_agent: args.user_agent,
            ..ClientConfig::default()
        };
        // Legacy Rest accepts both decoded and escaped route components. Normalize
        // once here; native callers already build encoded routes without this adapter.
        let mut url = crate::identity::endpoint(&args.server, &[])?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| Error::new(crate::ErrorKind::InvalidInput, "Invalid server URL"))?;
            for part in args
                .path
                .replace('\\', "/")
                .split('/')
                .filter(|p| !p.is_empty())
            {
                let decoded = percent_encoding::percent_decode_str(part)
                    .decode_utf8()
                    .map_err(|error| {
                        Error::with_source(
                            crate::ErrorKind::InvalidInput,
                            "Invalid UTF-8 listing route",
                            error,
                        )
                    })?;
                segments.push(&decoded);
            }
        }
        let prefix = crate::identity::endpoint(&args.server, &[])?
            .path()
            .trim_end_matches('/')
            .to_owned();
        let path = url
            .path()
            .strip_prefix(&prefix)
            .unwrap_or(url.path())
            .trim_start_matches('/')
            .to_owned();
        Ok(Pager::new(config, kind, args.server, path, vec![]))
    })();
    match result {
        Ok(pager) => Handle {
            pager: Some(pager),
            error: None,
        },
        Err(error) => Handle {
            pager: None,
            error: Some(error.to_string()),
        },
    }
}
/// # Safety
/// Input identifies `len` readable bytes, null only if empty. Returns an opaque
/// unique handle, freed once by `gz_fuel_rs_pager_free`; never dereference it in C++.
#[cfg_attr(not(test), unsafe(no_mangle))]
pub unsafe extern "C" fn gz_fuel_rs_pager_new(
    input: *const u8,
    len: usize,
) -> *mut std::ffi::c_void {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if input.is_null() && len != 0 {
            return Handle {
                pager: None,
                error: Some("Null pagination input".into()),
            };
        }
        // SAFETY: readable bytes are guaranteed by the caller; null-empty handled separately.
        let input = if input.is_null() {
            &[]
        } else {
            // SAFETY: the caller guarantees this non-null readable byte range.
            unsafe { slice::from_raw_parts(input, len) }
        };
        create(input)
    }))
    .unwrap_or_else(|_| Handle {
        pager: None,
        error: Some("Pagination initialization panicked".into()),
    });
    Box::into_raw(Box::new(result)).cast()
}
/// # Safety
/// Handle is live, returned by pager_new, exclusively accessed for this call.
/// Returned buffer is owned by Rust and must be freed unchanged exactly once.
#[cfg_attr(not(test), unsafe(no_mangle))]
pub unsafe extern "C" fn gz_fuel_rs_pager_next(handle: *mut std::ffi::c_void) -> FuelBuffer {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() {
            return json!({"error":"Null pagination handle"});
        }
        // SAFETY: unique live handle is guaranteed by the caller; no reference escapes.
        let handle = unsafe { &mut *handle.cast::<Handle>() };
        if let Some(error) = handle.error.take() {
            return json!({"error":error});
        }
        let Some(pager) = handle.pager.as_mut() else {
            return json!({"value":[]});
        };
        match pager.next_page() {
            Ok(items) => json!({"value":items.unwrap_or_default()}),
            Err(error) => json!({"error":error.to_string()}),
        }
    }))
    .unwrap_or_else(|_| json!({"error":"Pagination operation panicked"}));
    crate::owned_buffer(serde_json::to_vec(&result).expect("JSON values are serializable"))
}
/// # Safety
/// Handle is null or is the unchanged unique result of pager_new, not freed yet;
/// no concurrent operation may use it during or after this call.
#[cfg_attr(not(test), unsafe(no_mangle))]
pub unsafe extern "C" fn gz_fuel_rs_pager_free(handle: *mut std::ffi::c_void) {
    if !handle.is_null() {
        // SAFETY: unique allocation provenance/lifetime required by caller.
        drop(unsafe { Box::from_raw(handle.cast::<Handle>()) });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    unsafe fn next(handle: *mut std::ffi::c_void) -> serde_json::Value {
        // SAFETY: callers in this test own the live handle; buffer is read before freeing.
        unsafe {
            let buffer = gz_fuel_rs_pager_next(handle);
            let value =
                serde_json::from_slice(slice::from_raw_parts(buffer.data, buffer.len)).unwrap();
            crate::gz_fuel_rs_free(buffer);
            value
        }
    }
    #[test]
    fn opaque_lifetime_and_invalid_input_contracts() {
        // SAFETY: handles are used sequentially and freed once, pointers to JSON are live.
        unsafe {
            assert!(next(std::ptr::null_mut())["error"].is_string());
            for (input, len) in [(std::ptr::null(), 0), (std::ptr::null(), 1)] {
                let handle = gz_fuel_rs_pager_new(input, len);
                assert!(next(handle)["error"].is_string());
                assert_eq!(next(handle)["value"], json!([]));
                gz_fuel_rs_pager_free(handle);
            }
            for input in [
                br#"{"server":{},"path":"models","kind":"wrong"}"#.as_slice(),
                br#"{"server":{"url":"mailto:a"},"path":"models","kind":"models"}"#,
                br#"{"server":{},"path":"%ff/models","kind":"models"}"#,
            ] {
                let handle = gz_fuel_rs_pager_new(input.as_ptr(), input.len());
                assert!(next(handle)["error"].is_string());
                gz_fuel_rs_pager_free(handle);
            }
            gz_fuel_rs_pager_free(std::ptr::null_mut());
        }
    }
    #[test]
    fn paginates_an_owned_cursor_and_fuses_network_errors() {
        use crate::client::tests::Server;
        for (status, route) in [
            (200, "A B/collections/My?Collection/worlds"),
            (200, "A%20B/collections/My%3FCollection/worlds"),
            (503, "A B/collections/My?Collection/worlds"),
        ] {
            let server = Server::new(move |request| {
                assert!(request.contains("A%20B/collections/My%3FCollection/worlds"));
                (
                    status,
                    vec![("Link".into(), "</old>; rel=prev".into())],
                    br#"[{"name":"world","owner":"owner"}]"#.to_vec(),
                )
            });
            let input = serde_json::to_vec(
                &json!({"server":{"url":server.url},"path":route,"kind":"worlds"}),
            )
            .unwrap();
            // SAFETY: valid input bytes and unique live handle, freed once after use.
            unsafe {
                let handle = gz_fuel_rs_pager_new(input.as_ptr(), input.len());
                let result = next(handle);
                if status == 200 {
                    assert_eq!(result["value"][0]["name"], "world")
                } else {
                    assert!(result["error"].is_string())
                }
                assert_eq!(next(handle)["value"], json!([]));
                gz_fuel_rs_pager_free(handle);
            }
        }
    }
}
