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
