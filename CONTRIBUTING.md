# Contributing

Submit changes through a pull request. Explain the problem, resulting behavior,
and validation; link an issue when one exists. Keep changes focused enough to
review. Contributions are provided under this repository's Apache-2.0 license;
retain upstream notices and attribution. Report vulnerabilities through
[SECURITY.md](SECURITY.md).

## Build and test

See [README.md](README.md) for native dependencies and C++ setup, and
[docs/CI.md](docs/CI.md) for the enforced merge gates. Rust 1.92 is the declared
minimum and the pinned validation toolchain. Validation scripts require Python
3.11 or newer. Install `rustfmt`, `clippy`, and `llvm-tools-preview`, then run:

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
