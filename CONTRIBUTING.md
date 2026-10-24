# Contributing

Submit changes through a pull request. Explain the problem, resulting behavior,
and validation; link an issue when one exists. Keep changes focused enough to
review. Contributions are provided under this repository's Apache-2.0 license;
retain upstream notices and attribution. Report vulnerabilities through
[SECURITY.md](SECURITY.md).

Use `Copyright (C) YEAR Open Source Robotics Foundation` and
`SPDX-License-Identifier: Apache-2.0` in new project source files. Preserve the
original copyright years and notices in retained upstream files. Keep `LICENSE`
and `NOTICE` included in source packages and native installations.

## Build and test

See [README.md](README.md) for native dependencies and C++ setup, and
[docs/CI.md](docs/CI.md) for the enforced merge gates. Rust 1.92 is the declared
minimum and the pinned validation toolchain. Validation scripts require Python
3.11 or newer and the protobuf compiler (`protobuf-compiler` on Ubuntu) for schema differential tests; ordinary Rust builds do not need it. Install `rustfmt`, `clippy`, and `llvm-tools-preview`, then run:

```sh
scripts/install-hooks.sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
scripts/coverage.sh
scripts/parity.sh
cargo test --locked --doc
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps
python3 scripts/check_versions.py
```

The parity script checks public headers against pinned upstream sources, runs
the original and supplementary C++ suites, and checks C++ facade coverage.
Both production Rust and C++ line coverage must remain strictly above 95%.
Some parity tests contact public Fuel services; an outage is a failed check to
investigate and rerun, not a reason to mark the tests successful or skip them.
Record commands, results, and any checks you could not run. Local hooks help
catch mistakes; protected-branch CI remains the merge authority.

Behavior changes and bug fixes need tests that exercise the contract, including
relevant failure cases. Preserve upstream test inputs, assertions, and semantics;
add supplementary tests for intentional corrections. Do not lower thresholds,
exclude difficult production paths, or disable tests to make a change pass.
Documentation-only changes do not require artificial tests.
Public Rust API items must document their behavior, defaults and relevant errors.
Missing API documentation is a lint warning, rejected by required Clippy and
rustdoc checks with warnings denied.

## Compatibility and safety

Preserve the retained public C++ declarations and the pinned upstream API.
The C ABI is internal, but changes must keep both sides consistent. Document
ownership, lifetime, bounds, nullability, and threading assumptions for unsafe
operations. Keep unsafe blocks small and explain why each is sound. Panics and
C++ exceptions must not cross the FFI boundary.

Treat downloaded archives, filenames, URLs, metadata, and server responses as
untrusted. Preserve extraction containment, atomic cache replacement, bounded
work, certificate verification, and credential isolation across origins. Add
regression coverage when changing these boundaries.

Request independent review for substantial security, unsafe/FFI, or compatibility
changes and address findings before merging. The configured branch policy does
not impose a human approval count; see [docs/CI.md](docs/CI.md).

## Dependencies and automation

Commit intentional `Cargo.toml` and `Cargo.lock` changes together, explain the
need for new dependencies, and review their maintenance and license obligations.
Use the pinned audit tool:

```sh
cargo install cargo-deny --version 0.20.2 --locked
cargo deny --locked check
```

Do not add blanket advisory ignores or blanket license allowances. An exception
must identify the affected advisory/crate, explain exposure and mitigation, and
state a review deadline. Keep dependencies from approved sources and pin any
approved Git dependency to a full revision.

The Gazebo source revisions in `scripts/ci/dependencies.lock` are also reviewed
dependencies. Check upstream fixes, compatibility and native license obligations
when updating them and before preparing a release. OS dependencies are installed
from the hosted runner's package repositories on every CI run.

GitHub Actions use full commit SHAs with readable version comments. Review
Dependabot updates through the same checks as other code. Keep workflow token
permissions minimal and never expose secrets to untrusted pull-request code.
Do not change required check names without updating branch protection.

Release preparation must follow the release documentation. Publishing artifacts,
creating a release, or tagging a release requires an explicit release request;
ordinary implementation work does not authorize publication.

## Rewrite progress reporting

Repository agents maintain this project's record on the public
[rewrite dashboard](https://gazebosim-rs.github.io/rewrite-progress/). Read the
[status contract](https://github.com/gazebosim-rs/rewrite-progress/blob/main/status-contract.md)
before reporting. Update the record when current tasks, accepted work, remaining
scope, blockers or verification evidence change. Publish only this owner's
`data/gz-fuel-tools-rs.json` on the tracker's `status` branch through its updater;
do not edit another repository's report.

Keep the local JSON report under the ignored `coverage/` directory. Use a current
UTC timestamp, public evidence links and exact measured counts; unknown
measurements are `null`. Identify the measured revision and link CI for that same
revision. Explain test and coverage scope, including separate Rust and C++ results,
and never present working-tree measurements as proof of a pushed commit's CI.
Exclude credentials, private paths, environment dumps and private user data.

With a sibling checkout of `rewrite-progress`, validate and then publish through
the existing authenticated `gh` session:

```sh
python3 ../rewrite-progress/scripts/update-status.py gz-fuel-tools-rs coverage/rewrite-progress.json --dry-run
python3 ../rewrite-progress/scripts/update-status.py gz-fuel-tools-rs coverage/rewrite-progress.json
```

Report standalone Fuel readiness separately from consumer integration. `FUEL-01`
belongs to `gz-sim-rs`; completion of this library does not establish that
integration's acceptance, and this repository must not mark that packet complete.
