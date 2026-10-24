# Rewrite worklog

## 2026-10-24 — baseline and design

- Located the local upstream source and existing built Gazebo dependency tree.
- Baseline source is commit `6ac62b01d7d8af3ab23fd2239728a791704a3f09`, 12.0.0-pre2.
- Reviewed all 17 public class/function headers and the private iterator/cache
  interfaces used by upstream tests. Retaining headers and PIMPL contracts.
- An independent compatibility agent ran the 15 deterministic original suites:
  all passed against the original implementation. The remaining suites involve
  remote services and the installed `gz` command.
- Began upstream issue review before implementation. GitHub's HTML issue page
  displayed only a subset; a separate reviewer is fetching the complete backlog.
- Wrote IMPLEMENTATION_PLAN.md before production implementation. Selected a
  Rust library with C++ facade and an internal length-safe JSON C ABI to avoid
  coupling the Rust API to Gazebo C++ dependency types.
- Copied unchanged Apache-licensed public declarations and private test-facing
  declarations into the replacement; no original Fuel algorithms are reused.

## Implementation pass

- Implemented values/config/JSON/result/helper operations in Rust; C++ methods
  preserve public signatures and reference-returning storage.
- Implemented verified HTTPS requests, multipart forms and byte-preserving HTTP
  bodies, ZIP compression/extraction, safe cache scans/saves, URI rewrites and SDF
  selection. Added local HTTP, invalid archive, filesystem and rollback tests.
- Independent cache review identified overwrite data-loss risk and cache authority
  spelling mismatch. Both were addressed with backup/rollback and matching URI
  authority behavior.
- Implemented shared model/world client URL parsing, details, downloads, referrals,
  dependency traversal, forms, delete/patch/update, cached files and bounded batch
  workers. Cycle detection replaces recursive unbounded dependency traversal.
- Original public headers remain unchanged; facade implementations use internal
  JSON marshalling. No original Fuel implementation is compiled or linked.
- Cargo dependencies and matching LLVM coverage tools required writes to the
  read-only tool cache; requested tool escalation for these environment changes.

## First acceptance runs and independent review

- Corrected the parity harness to define Gazebo's TESTING_PROJECT_SOURCE_DIR,
  required for upstream temporary-directory fixtures, and prepend the replacement
  library directory to the test runtime path. This prevents accidentally testing
  the installed upstream implementation.
- All 15 unchanged deterministic upstream suites pass against the replacement.
- Unchanged live FuelClient and Interface suites pass against public Fuel servers.
  The first gz_src run passed individual model/world commands and world collections,
  but failed six model-collection cases; investigating rather than skipping them.
- Added typed native Rust configuration, identifiers, results, lazy listings and
  complete client operations; standalone Rust CLI shares the same command logic.
- Added tracked native pre-commit hook and pre-commit framework configuration.
  Initialized the new output folder as a repository and activated core.hooksPath.
- Independent reviews found and fixed URI authority spelling, transactional cache
  replacement, explicit-version fallback, collection case preservation, and HTTP
  path component encoding. Added regression tests for these behaviors.

## Review fixes and collection regression diagnosis

- Preserved archives as native Rust byte buffers through transport and extraction;
  JSON byte marshalling is confined to the C++ compatibility adapters.
- Added a redirect policy that refuses HTTPS downgrades and cross-origin redirects
  carrying Private-token, with local two-origin and redirect-loop regressions.
- Added canonical escaped URL/cache identities and escaped file lookup checks.
  Native Rust listings now yield Result items and expose transport/JSON errors.
- Native batch results retain resolved versions; C++ ModelResult retains its
  historical requested identifier. Scheduling deduplicates by resource identity,
  not incidental metadata, and handles zero jobs without hanging.
- Diagnosed the six live model-collection failures: real teambase/backpack archives
  contain leading whitespace before XML declarations. TinyXML2 accepted this;
  roxmltree required trimming for parsing. Fixed metadata/SDF parsing while
  retaining original text offsets for URI rewriting. Added a deterministic archive
  fixture. A temporary live probe verified both models, then was removed.
- Added supplementary C++ API tests to cover previously untested overloads,
  binary/multipart HTTP, cache saves, reference returns, copy/move and iterator
  boundaries. Rust core and C++ facade coverage are gated separately above 95%.
- Independent production install/consumer checks found relocation, multiarch Ruby
  launcher paths and umbrella version-macro omissions. Fixed these packaging
  issues and added native gz-tools launcher integration.

## Final acceptance

- All 21 C++ suites pass against the final replacement: all 19 original suites,
  including the six upstream-disabled cases explicitly enabled, plus both added
  compatibility suites. Zero failures in the final complete run.
- All 47 Rust unit tests, two standalone CLI integration tests and two Python
  coverage-checker regressions pass. No tests ignored in the final Rust run.
- Production line coverage: Rust 2583/2656 = 97.2515%; C++ 1319/1359 = 97.0567%.
  Both independently exceed the strict 95% hooks/CI threshold. FFI profiling names
  are distinct in test builds, eliminating LLVM cross-binary counter collisions.
- Rust formatting and all-target Clippy with denied warnings pass. All 17 original
  public C++ headers are unchanged. Original gz-fuel-tools source stays clean.
- Verified fresh production installs, external C++ consumers, relocation,
  multiarch/lib64 layouts, pkg-config, umbrella version macros and gz-tools launch.
- Final review/verification records and usage/build documentation are included.
  The project has an active tracked pre-commit hook; no commit or publication was
  requested or performed.

## Pull request CI and merge enforcement

- Inspected the first GitHub Actions run: Rust passed, C++ was indefinitely queued
  because the workflow requested a nonexistent self-hosted Gazebo runner.
- Replaced that requirement with hosted Ubuntu 24.04 runners and a reproducible,
  cached build of pinned gz-cmake/utils/math/common/msgs dependencies.
- Added an always-running aggregate CI check that requires both language jobs to
  succeed. Every pull request runs the workflow without file-path filters.
- Applied main-branch protection requiring PRs, an up-to-date branch and the
  GitHub Actions-owned CI gate. Administrator enforcement is enabled, with force
  pushes and deletion disabled. Saved the policy as versioned JSON.
- Canceled the obsolete queued workflow. Validating the updated workflow through
  its own pull request before merging the configuration.

## Development lifecycle review

- CI PR #1 passed all hosted jobs and merged through the required check, without
  administrator bypass. Verified that live main protection still requires PRs,
  current successful CI and administrator enforcement.
- Ran separate reviews of implementation correctness, dependency/security policy,
  and build/package/release readiness. The review found metadata/file ambiguity in
  multipart uploads, cross-origin dependency credential forwarding, swallowed
  listing failures, missing CLI listing headers and incomplete cached dependency
  traversal. Added focused fixes and deterministic regressions.
- Enabled repository secret scanning and push protection, Dependabot security fix
  PRs and private vulnerability reporting. Added contributor, security and agent
  guidance and weekly dependency/action update configuration.
- Pinned action implementations to reviewed full revisions, added workflow linting
  and documentation checks, and extended the aggregate merge gate to include
  dependency policy. Release preparation remains separate from actual publishing.
- Initial RustSec scan found unmaintained YAML dependencies and advisories against
  the old ZIP dependency. Replacing affected dependencies instead of ignoring the
  findings; final tests and audit evidence are recorded after validation below.
- Replaced serde_yaml/unsafe-libyaml with maintained serde-saphyr and upgraded
  ZIP to 9.1.0; all four cargo-deny domains pass with no advisory exceptions.
- Full upstream parity exposed Fuel's header-advertised pagination completion and
  empty-token YAML quoting. Preserved both contracts while retaining failure
  propagation. Independent review added repeated Link and quoted-parameter cases.
- Release checks pass on clean temporary snapshots, including crate verification,
  source-only C++ builds, moved-prefix library/CLI consumers and Ruby dispatch.
  Dispatcher YAML uses an absolute path and is refreshed by reinstalling after
  moving, which is now documented and exercised.
- Local final validation: 55 Rust unit tests, two CLI tests, four Python tests;
  all 21 C++ suites pass. Rust coverage 2736/2815 = 97.1936%, C++ coverage
  1319/1359 = 97.0567%. Formatting, Clippy and documentation checks pass.
  Hosted CI independently repeats the gates on the protected pull request.
- Hosted PR validation passed Rust, dependency/workflow policy and original C++
  parity, then rejected packaging: unanchored Cargo filename globs matched README
  and license files in the ignored nested upstream checkout. The required gate
  failed and blocked merging. Anchoring distribution paths and checking both the
  package file list and produced archive prevents this contamination without
  allowing dirty packaging or weakening the clean-checkout rule.
- The packaging regression now reproduces an ignored nested Git checkout using
  real Cargo. All eight Python validation tests pass; a clean snapshot containing
  the real upstream checkout produces and verifies exactly 26 native crate files,
  with list/archive agreement and no upstream paths. No --allow-dirty is used.

## Copyright attribution alignment

- Confirmed upstream uses Open Source Robotics Foundation as the copyright holder.
  Retained upstream public and private declarations keep their original notices
  and years; all public headers remain byte-for-byte identical.
- Added matching 2026 copyright and Apache-2.0 SPDX notices to rewritten project
  sources and supporting scripts, plus a repository-wide NOTICE and README notice.
- Require NOTICE in the native Rust crate and install LICENSE/NOTICE with the C++
  facade. Package tests reject missing attribution, and release checks compare
  installed legal documents with the source archive.

## Adversarial review corrections: cache, archives, and package consumers

- Replaced JSON-based native cache operations with typed identifiers, configuration,
  readers, and cache results. Exact version lookup first touches its direct cache path;
  legacy case spellings are resolved only through matching owner/resource siblings,
  and tip compares their numeric versions. Cache identity uses
  the same ASCII normalization as parsing, preserving non-ASCII spelling.
- Resource archives now enforce compressed size, raw central-directory entry
  count, path depth, individual expanded size, aggregate expanded size, and
  metadata/rewrite limits. Compression propagates directory enumeration errors
  instead of returning a successful archive with omitted files.
- Validate complete model metadata and the main SDF in private staging before
  publishing. Asset references use the shared URL builder and encode decoded
  identity/file components; escaped URI suffixes retain their meaning.
- Publish resolved versions under cross-process resource-parent directory locks.
  Linux replacement uses atomic directory exchange, so readers retain an existing
  resource path throughout publication. Metadata readers hold a shared lock
  across multiple file reads; Linux uses read-only directory descriptors, so legacy
  read-only caches require no writable lock sidecars. Unsupported replacement platforms/filesystems fail
  explicitly while preserving the existing cache instead of using a visibility
  gap and best-effort rollback.
- Added focused regressions for rejected replacement metadata, Unicode identity,
  escaped filenames, archive budgets, unreadable compression directories, exact
  lookup, and simultaneous child-process publication with a concurrent reader.
  Existing project-authored synthetic model fixtures were made structurally valid
  for their original success-path assertions; retained upstream fixtures and
  assertions were not changed.
- Restored upstream CMake core/requested/all/plain targets, compatibility library
  and include variables, the all-component package, and relocated include values.
  Release checks now compile and execute seven installed consumer variants.
  Generated headers and launchers preserve their timestamps when unchanged, and
  metadata schema assets participate in Cargo rebuild dependencies.
- The updated CMake configuration has been generated locally. Full integrated
  Rust/C++ checks and coverage are pending completion of the coordinated changes;
  earlier measured coverage is historical evidence, not validation of this tree.

## 2026-10-24 — clean-context critical review corrections

- Started `fix/review-corrections` from protected main after the owner authorized
  all review fixes. Recorded the complete finding/acceptance matrix in
  REVIEW_FIX_PLAN.md before integration.
- Assigned independent implementation owners for domain/schema behavior,
  cache/archive transactions, and transport/binary FFI. Integration owns the
  native API, shared pager and C++ iterator cursor lifetime.
- Replaced native JSON operation marshalling with a typed ClientCore and native
  PathBuf arguments. Added categorized errors retaining causes and HTTP status.
- Added finite configurable budgets for responses, aggregate downloads, archive
  entry count/depth/expansion, dependency graphs, listing bytes/resources/pages,
  connection setup, optional whole requests and stalled reads.
- Implemented a single pager with typed identity fingerprints, total/Link
  consistency checks, non-adjacent cycle rejection and fused failures. C++ owns
  an opaque Rust cursor through a narrow RAII adapter.
- Initial integrated Rust run: 63 of 74 unit tests passed. Eleven failures exposed
  configuration error-path compatibility, metadata validation whitespace,
  dependency fixtures and URL suffix decoding. These are being corrected;
  acceptance and coverage are not yet claimed for this working tree.

### Cache/package correction validation and independent review

- Cache/archive ownership: `src/io.rs`, `src/cache_tests.rs`; package correction
  ownership: `CMakeLists.txt`, package configuration templates, and installed
  consumer fixtures. Native cache paths perform no JSON serialization; legacy
  adapters recover configured YAML limits before entering the typed layer.
- Linux readers and writers now lock the stable resource-parent directory rather
  than creating writable sidecars. A legacy read-only cache metadata lookup
  returns the upstream success result. Tip compares numeric versions across
  matching mixed-case directory spellings while successful explicit canonical
  lookups retain their direct path.
- Scoped current-tree validation passed: `cargo test --locked --lib io:: --
  --nocapture` (29 tests), `cargo clippy --locked --all-targets -- -D warnings`,
  and formatting checks for the cache/archive files. The concurrent regression
  uses four child processes performing 32 replacements each while another thread
  checks path availability and shared-locked multi-file consistency.
- Rebuilt the C++ facade and ran the unchanged upstream
  `FuelClientTest.CachedModel` and `FuelClientTest.CachedWorld`: both pass.
  Independently reran C++ reproductions for read-only dependency metadata and
  canonical-version-1 / legacy-mixed-case-version-9 tip lookup: they now return
  success and the version-9 physical path respectively.
- Installed package metadata was moved to another temporary prefix; all seven
  target/legacy-variable consumer variants configure, build, and execute.
  Generated header timestamps remain unchanged across a repeated CMake configure.
- Independent review of the typed domain, transport, binary bridge, and pager
  identified referral downgrade handling, resolved-version batch deduplication,
  multipart traversal accounting, chunked metadata limits, upload inactivity
  semantics, and preescaped legacy pager routes. Findings were returned to their
  implementation owners for fixes and focused regressions. Binary buffer/opaque
  pager ownership and C++ release guards were inspected directly.
- Full-tree parity, coverage, artifact, and dependency acceptance are coordinated
  separately; these scoped results do not replace the complete required gates.

## 2026-10-24 — typed domain, identity and metadata review corrections

- Replaced the native JSON operation dispatcher with `ClientCore` methods over
  typed configuration, identifiers, filesystem paths, transport requests and cache
  results. The legacy dispatcher now adapts into that same core. Shared pagination
  and configurable budgets apply to the CLI as well as native calls.
- Preserved server privacy in details and content-only edits; PATCH omits absent
  replacement description/license fields. Explicit CLI endpoints now retain their
  matching configured authentication. Kept upstream metadata/path upload failures
  distinct from PATCH transport failures at the compatibility boundary.
- Unified ASCII model/world normalization, case-preserving collections and escaped
  URL components. Configured server base paths round-trip through parsing, using
  the longest matching prefix. Configuration loading retains native `PathBuf`
  values and YAML budgets; legacy adapters recover limits hidden by C++ getters.
- Replaced the handwritten protobuf text parser with `prost-reflect` and typed
  `prost` messages using checked-in, attributed gz-msgs schemas and their descriptor.
  Ordinary Rust builds do not require protoc. Schema conformance tests compare a
  ten-case valid/invalid corpus with protoc, and reproduce the embedded descriptor.
  Main SDF validation accepts named model/actor resources and named worlds, rejects
  traversal/symlink main paths, and honors configured metadata byte limits.
- Bounded graph depth/count, aggregate download/referral bytes, referral hops and
  upload filesystem traversal. Referrals obey HTTPS downgrade policy. Batch walks
  deduplicate a resolved tip against explicit self-dependencies. Multipart files
  remain file-backed; non-UTF-8 native directories/config/cache paths are retained,
  while unrepresentable multipart asset filenames return a structured error.
- Verified upstream JSON numeric behavior with an isolated temporary executable
  compiling the pinned `JSONParser.cc` as an oracle. Scalar overflow preserves
  earlier identifier fields; array parsing omits the failed record; malformed
  license entries are skipped while valid entries before and after remain. No
  original Fuel implementation was added to production targets.
- Working-tree validation for this pass: `cargo test --locked --offline --lib`
  passed 106 tests; `cargo clippy --locked --offline --all-targets -- -D warnings`
  passed; a fresh native binary passed
  `python3 scripts/check_metadata_schema.py --binary target/debug/gz-fuel`; the
  schema Python unittest passed; `git diff --check` passed. These are local
  working-tree results, not evidence for a pushed commit or its CI. Final complete
  parity, package and fresh coverage evidence is recorded by the coordinating run.

### Final independent-review follow-up: batch budget aliases

The final reviewer reproduced a batch graph with two actual resources failing a
budget of two because the deduplication set also contained resolved-version alias
keys. Batch scheduling now counts deduplicated pending resources separately from
that alias set. The regression covers exact two-resource success versus a budget
of one, plus concurrent branches sharing a leaf and explicit ancestor-version
aliases across three waves (exact budget four succeeds; three fails before the
leaf request). `cargo test --locked --offline --lib batch_` passed all four scoped
batch tests; all-target Clippy with warnings denied and `git diff --check` passed.

### Final clean-context reviewer and transport memory evidence

- A final reviewer began without conversation history and inspected the native
  API, identity/schema implementation, client graph operations and transport.
  Public probes reproduced descriptor exhaustion, alias-budget overcounting and
  duplicate queued dependency growth. The first two fixes were independently
  inspected; queue correction is included in the final review pass.
- Multipart file handles now open lazily for the active part and close after its
  last declared byte. An isolated child with descriptor limit 128 successfully
  uploads 302 files, including binary bytes, from a non-UTF8 parent directory.
  The parent verifies that the intended child test actually ran.
- A Linux Release build with C++ coverage disabled measured 18,668 KiB peak RSS
  for a 1 MiB REST response (16,392 KiB for an empty response). The earlier review
  measured approximately 214,036 KiB for that response through JSON byte arrays.
  These are observed process measurements, not portable performance guarantees.
- Both public C++ binary regressions passed against that optimized library:
  every byte value survives request/response, and valid model/world ZIPs retain
  separate binary assets. No binary payload is encoded as a JSON number array.
- Upgraded Tokio to 1.53.3 to resolve the current audit finding without an ignore.
  The locked graph passes advisory, license, source and bans checks. Declared
  dependency minimum Rust versions remain below the repository's Rust 1.92.

### Final independent-review follow-up: dense dependency frontiers

Single and batch downloads now share a pending-resource queue that rejects known
and already-pending keys before cloning identifiers. Resolved numeric versions
remove their queued aliases. Pending keys have a linear allowance of twice the
resource budget, allowing unresolved tip/numeric pairs without counting them as
extra completed resources; actual scheduling retains its separate resource/depth
checks. A private queue regression traverses all 6,400 edges of an 80-node complete
cyclic graph with a high-water mark of exactly 80 pending identifiers. Public API
regressions exercise a 12-node cached complete graph (one root HTTP request), its
batch traversal (13 unique requests), and tip/numeric fanout fitting an actual
three-resource allowance. The complete native library suite passed 111 tests;
all-target Clippy with warnings denied, formatting and diff checks passed. These
are working-tree checks pending the final complete coverage/parity refresh.

### Coordinated review-correction acceptance

- Final reviewer approved the three additional corrections and independently
  reran dense frontier, exact batch budget, alias fanout and cached graph cases.
  No actionable findings remained in that review scope.
- Fresh full Rust validation passed 111 unit tests, two CLI integration tests and
  nine Python regressions. Production coverage is 4252/4447 = 95.6150%.
- Full CTest passed all 22 suites: 128 unchanged upstream cases and 15 additional
  cases, including inherited disabled service tests. C++ production coverage is
  1363/1405 = 97.0107%. All 17 original public headers match byte for byte; the
  upstream source checkout remains clean.
- Formatting, denied-warning Clippy, documentation, doctests, workflow syntax,
  metadata descriptor/protoc/CLI differential tests and cargo-deny checks pass.
  The tracked pre-commit hook remains active and the strict coverage policies
  have not changed. Main protection was verified against GitHub's live API.
- Preparing the committed candidate for clean source/crate installation and
  relocated consumer validation, followed by the required hosted PR checks.

### Clean committed package and installation validation

- `scripts/ci/release-checks.sh` passed for candidate commit
  `0a1ca052f52c8cd2dd74c8ae60d19dd92e381a24` from a clean checkout.
- Cargo verified the generated crate and its exact file list, including LICENSE,
  NOTICE and embedded metadata schemas. A clean source archive built the Rust
  library/CLI and C++ facade with tests disabled and no upstream source checkout.
- Installed and relocated the candidate; all seven CMake target/variable consumer
  variants passed. The separate pkg-config consumer, CLI version/help, Ruby
  dispatcher, relocated include/library paths and installed license files passed.
- Candidate archives, checksums and commit identity were generated only as local
  validation artifacts. No crates.io package, release tag or GitHub release was
  published. The protected PR's required hosted workflow repeats these checks
  against its final checkout before merge.
