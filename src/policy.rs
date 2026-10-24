// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Explicit finite budgets for untrusted resource work.
use serde::{Deserialize, Serialize};

/// Resource and transfer budgets. Byte counts are uncompressed unless named download.
/// Defaults allow large resources while bounding malformed inputs and graph work.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceLimits {
    /// Maximum bytes received by one download or compatibility REST response.
    pub max_download_bytes: u64,
    /// Maximum bytes received across one dependency or batch operation.
    pub max_total_download_bytes: u64,
    /// Maximum bytes in one metadata document or listing response.
    pub max_metadata_bytes: u64,
    /// Maximum entries in one ZIP archive.
    pub max_archive_entries: usize,
    /// Maximum number of path components in an archive entry or compression walk.
    pub max_archive_depth: usize,
    /// Maximum expanded bytes in one archive entry.
    pub max_archive_entry_bytes: u64,
    /// Maximum combined expanded bytes in one archive.
    pub max_archive_total_bytes: u64,
    /// Maximum unique resources visited by a dependency/batch operation.
    pub max_dependency_resources: usize,
    /// Maximum dependency graph depth, with the requested root at depth zero.
    pub max_dependency_depth: usize,
    /// Maximum pages in one listing.
    pub max_pages: usize,
    /// Maximum resources in one listing.
    pub max_listing_resources: usize,
    /// Maximum combined response bytes in one listing.
    pub max_listing_bytes: u64,
    /// Connection establishment timeout in milliseconds.
    pub connect_timeout_ms: u64,
    /// Optional whole-request timeout; `None` permits long progressing transfers.
    pub request_timeout_ms: Option<u64>,
    /// Optional per-read inactivity timeout in milliseconds.
    pub read_timeout_ms: Option<u64>,
}
impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_download_bytes: 1024 * 1024 * 1024,
            max_total_download_bytes: 8 * 1024 * 1024 * 1024,
            max_metadata_bytes: 16 * 1024 * 1024,
            max_archive_entries: 100_000,
            max_archive_depth: 128,
            max_archive_entry_bytes: 2 * 1024 * 1024 * 1024,
            max_archive_total_bytes: 8 * 1024 * 1024 * 1024,
            max_dependency_resources: 10_000,
            max_dependency_depth: 64,
            max_pages: 10_000,
            max_listing_resources: 1_000_000,
            max_listing_bytes: 256 * 1024 * 1024,
            connect_timeout_ms: 15_000,
            request_timeout_ms: None,
            read_timeout_ms: Some(30_000),
        }
    }
}
