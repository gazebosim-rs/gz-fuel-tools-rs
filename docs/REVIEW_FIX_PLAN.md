# Critical review correction plan

The clean-context review compared the rewrite with upstream commit
`6ac62b01d7d8af3ab23fd2239728a791704a3f09`. This follow-up implements
all seventeen findings and the associated simplifications. Public upstream
headers and original tests remain unchanged.

| Finding | Correction | Regression evidence required |
| --- | --- | --- |
| F01 | Preserve private server metadata; sparse content patches omit unspecified visibility | Captured PATCH fields and private JSON decode |
| F02 | Validate metadata and selected SDF in staging before committing cache entries | Invalid replacement preserves original bytes |
| F03 | Coordinate writers across processes; atomically exchange replacement directories | Concurrent processes and uninterrupted readers |
| F04–F05 | Share a bounded pager across CLI, Rust and C++; honor Link completion and detect all repeated pages | World/model terminal links and A/B/A cycles |
| F06 | Match explicit server URLs to configured credentials | Listing/upload token capture |
| F07–F09 | Share identity normalization and escaped URL building; preserve collection spelling | Unicode names, reserved characters and case-sensitive collections |
| F10 | Check wire numeric bounds before C++ conversions | Boundary values and upstream-compatible partial decode |
| F11 | Parse protobuf text using the original message descriptors | Differential protoc corpus and nested schema failures |
| F12 | Restore upstream CMake aliases and variables | Installed, relocated consumers and repeated component lookup |
| F13 | Stream multipart file contents with pooled HTTP clients | Multipart bytes, connection reuse and production memory probe |
| F14 | Propagate directory traversal failures during compression | Unreadable subtree fails rather than losing files |
| F15 | Bound transfers, expansion, graphs and listings; configure timeouts | Known/streamed body limits, ZIP limits, graph bounds and progressing transfers |
| F16 | Transfer binary C++ bodies independently of JSON control messages | All byte values, ownership/error paths and production memory probe |
| F17 | Keep native filesystem paths as PathBuf throughout operations | Non-UTF8 upload, configuration and cache paths |

Final independent review additionally verifies low-descriptor multipart uploads,
actual-resource graph accounting independent of resolved aliases, and deduplicated
pending dependency frontiers for dense cyclic graphs.

Implementation ownership is split across metadata/domain behavior, cache/archive
transactions, transport/binary FFI, and integration/pagination. Each area receives
independent cross-review. JSON remains at HTTP and C++ compatibility boundaries;
native operations use typed configuration, identifiers, results and structured
errors. Exact cache lookup uses direct paths rather than scanning the cache.

Acceptance requires formatting, denied-warning Clippy, documentation checks,
dependency/license/advisory checks, unchanged-header checks, all original tests,
new deterministic regressions, production Rust and C++ coverage each strictly
above 95%, packaging consumers, and protected GitHub PR CI. The coverage policy,
required check and branch protection must remain enabled. No release is created.
