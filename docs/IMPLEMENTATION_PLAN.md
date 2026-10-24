# gz-fuel-tools Rust rewrite implementation plan

Baseline: gz-fuel-tools commit `6ac62b01d7d8af3ab23fd2239728a791704a3f09`
(12.0.0-pre2). Work began 2026-10-24, America/Los_Angeles.

## Acceptance criteria

- A self-contained Rust implementation in `gz-fuel-tools-rs`, without linking or
  compiling the original Fuel implementation.
- Retain existing public C++ headers and signatures, including reference-returning
  APIs and private factory interfaces exercised by upstream tests.
- Compile and run every existing upstream test against the replacement. Distinguish
  deterministic failures from unavailable external services; never silently skip a
  failing test or describe an unrun test as passing.
- Greater than 95% coverage of production Rust code, with meaningful success,
  malformed-input, network-error, archive and filesystem tests. Measure C++ facade
  coverage separately so it cannot hide missing compatibility verification.
- Formatting, lint, tests and strict coverage enforced by pre-commit and CI.
- Independent reviewers check functionality, compatibility, test integrity and
  maintainability. Record findings, fixes and unresolved limits.

## Architecture

Use a Rust library containing shared resource identifiers, configuration, JSON,
HTTP, archive, cache and client operations. Models and worlds share algorithms
parameterized by resource kind. Keep transport, parsing and storage separate and
use local HTTP fixtures for deterministic network tests. Avoid process-global
mutable state and shell-based implementations of Fuel operations.

A small C ABI transfers length-safe JSON messages between Rust and C++ facade
implementations. This internal ABI is not the consumer API. Existing C++ headers
remain the consumer contract; C++ adapts Gazebo URI types, containers, virtual
interfaces, iterator handles and stable reference returns. All substantive Fuel
algorithms belong in Rust. Binary data travels as byte arrays, never lossy text.
The C++ facade links the Rust static library, never the original Fuel library.

## Work sequence

1. Inventory all public methods, private test-facing interfaces, fixtures and
   tests. Record baseline test results. Review every upstream open issue and
   select changes suitable for the conversion, with linked rationale.
2. Build the Rust crate, explicit error boundary, generated export/config headers,
   CMake integration and unchanged upstream test harness.
3. Implement values, config, result semantics, helpers and JSON parsing/serialization.
4. Implement safe binary ZIP IO, verified HTTPS, HTTP methods, multipart uploads,
   cache scanning/matching/atomic saving and SDF selection.
5. Implement shared URL parsing, listing/pagination, downloads, dependency handling,
   upload/patch/delete/update and parallel batch operations; provide all C++ facade
   methods, iterator factories, interface functions and command entry points.
6. Run original suites, add local fixtures for network/error regressions and test
   issue fixes. Resolve compatibility failures without weakening assertions.
7. Measure coverage, fill meaningful gaps until >95%, installable pre-commit and
   CI gates. Independently review and resolve findings. Document installation,
   Rust/C++ usage, intentional changes and measured verification results.

## Review ownership

Separate agents audit the upstream issue backlog and C++ compatibility first.
Implementation is split across value/config/JSON, IO/cache, and client/build
areas. Agents subsequently review code they did not author; the primary agent
integrates and reruns acceptance checks. No publication or upstream modifications
are part of this request.
