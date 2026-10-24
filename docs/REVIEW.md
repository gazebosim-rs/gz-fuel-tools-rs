# Architecture and compatibility review

Reviewed against upstream commit `6ac62b01d7d8af3ab23fd2239728a791704a3f09`
(12.0.0 pre2). This review records cross-agent findings and their disposition;
final build, test and coverage measurements belong in `WORKLOG.md`.

## Review responsibilities

- The compatibility reviewer inventoried every public header and the private
  factory interfaces referenced by upstream tests, established the original
  deterministic test baseline, implemented the value/configuration/JSON layer,
  and reviewed client, native API, FFI and coverage boundaries.
- The IO reviewer independently reviewed the value layer and implemented and
  tested archive, transport and cache behavior. The compatibility reviewer then
  reviewed that implementation and the additional C++ compatibility tests.
- The issue/command reviewer examined upstream issues and metadata/command/
  pagination behavior. The compatibility reviewer cross-reviewed those interfaces.
- The primary implementer integrated the facade, build, native API and quality
  gates, and ran the complete replacement test and coverage commands.

These are cross-reviews; a reviewer is not represented as an independent reviewer
of code that they originally implemented.

## Findings resolved during review

| Finding | Resolution and evidence |
| --- | --- |
| Tests reference private iterator factories and cache/model storage types in addition to installed headers. | Preserved the test-facing declarations and exercised unchanged upstream sources against the replacement library. |
| A system-installed Fuel library could be selected before the replacement at runtime. | Parity harness explicitly selects the replacement library and build-local CLI. The original Fuel implementation is not linked. |
| Upstream fixture setup needs `TESTING_PROJECT_SOURCE_DIR`. | Added the required test definition; the initial fixture crashes were build-harness failures rather than accepted test exclusions. |
| YAML server URIs have historical authority-mode semantics. | Preserved the flag through value marshalling and used consistent cache identity; added port/authority compatibility checks. |
| Removing a cached resource before installing its replacement could lose working data. | Stage the new resource and restore the previous directory if publishing fails; IO regression tests cover rollback. |
| URL-escaped resource names and literal spaces could create separate cache identities. | Decode resource components once, validate canonical components, and encode request components once. Reject traversal, embedded separators, NUL and invalid UTF-8. |
| Cached file lookup joined percent escapes as literal filename characters. | Decode the file path before lookup, reject absolute/traversal paths and confirm the resolved file remains under the resource directory. Tests cover filenames containing spaces and symlink escape. |
| Typed batch results could report the requested `tip` instead of the concrete downloaded version. | Preserve the resolved identifier in native results and retain the requested identifier separately for the historical C++ result tuple. |
| Native listing treated transport failures as an empty collection. | Listing yields `Result<ResourceId, Error>`, emits an error once and then remains exhausted. Tests cover transport/status, malformed data, repeated pages and counter overflow. |
| Metadata conversion hardcoded an SDF format version. | Preserve the declared format and validate metadata shapes; command conversion regression tests cover roundtrips and malformed declarations. |
| Rust panic/error envelopes become C++ exceptions in the bridge. | Catch exceptions around every exported integer C command hook, emit a diagnostic and return failure rather than unwinding through a foreign C caller. The Rust buffer boundary separately catches panics and documents allocation ownership. |
| A coverage gate must not count test code or retain stale profiles. | Use matching toolchain LLVM tools, fresh profiles, exact test-module/source discovery, fail on missing production files, and require measured production line coverage strictly greater than 95%. Python regressions check literal/comment masking and external test modules. |

## Evidence and scope

The original deterministic baseline passed all 15 selected upstream suites before
implementation. Replacement validation uses the unmodified upstream C++ tests,
plus local HTTP regressions for status handling, uploads, downloads, dependencies,
cache behavior, pagination, binary bodies and every supported HTTP method. An
independent stored-ZIP fixture in `tests/compatibility.cc` avoids using the archive
implementation under test to generate every expected input.

The native Rust API has typed public configuration/resource/result values. Shared
internal dispatch and serialized C++ adaptation are implementation details. The
facade preserves C++ source declarations and reference behavior; downstream code
must be rebuilt. Binary ABI interchangeability with an existing installed Fuel
library is not asserted.

Validation is on Linux. POSIX socket compatibility tests do not establish Windows
or macOS support. Live upstream service tests remain sensitive to server changes;
failures must be reported and investigated, never converted into silent skips.
Catch-all FFI exception handlers protect a failure boundary; normal argument and
network error paths are tested without adding production fault-injection APIs.

## Development lifecycle review

Three separate reviews examined implementation correctness, CI/security policy,
and package/release readiness. The primary implementer cross-reviewed code fixes;
policy and code reviewers also reviewed each other's pagination and workflow work.

Resolved findings include multipart metadata/file ambiguity, caller credential
forwarding to foreign dependency origins, arbitrary authentication headers on
redirects, failure-swallowing aggregate listings and skipped cached descendants.
Full parity exposed serializer quoting and pagination-boundary compatibility
regressions; both were corrected without changing upstream tests. Further review
caught repeated Link fields and quoted relation handling that could truncate
results; native and CLI regressions cover them.

The aggregate CI gate requires success from every dependency, including canceled
or skipped jobs. Actions are pinned, PR tokens are read-only, checkout credentials
are disabled, and publishing is separate from validation. Dependency findings
were resolved by upgrades/replacement, with no advisory ignores. Package checks
build from a clean source archive and run relocated external consumers. Gazebo's
absolute dispatcher manifest is explicitly refreshed after moving an installation;
this limitation is documented and tested rather than implied to be relocatable.
