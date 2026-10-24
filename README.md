# Gazebo Fuel tools in Rust

Rust implementation of the gz-fuel-tools 12.0.0-pre2 API, with a native Rust
client/CLI and a source-compatible C++ facade. The original Fuel implementation is
never built into or linked by this library. Public C++ declarations are retained
from upstream commit `6ac62b01d7d8af3ab23fd2239728a791704a3f09` under Apache-2.0.

Final validation: all 21 C++ suites (including upstream-disabled service tests),
47 Rust unit tests, two CLI integration tests and two coverage-checker tests pass.
Production line coverage is **97.25% Rust** and **97.06% C++**. See the
[verification report](docs/VERIFICATION.md) for evidence and scope.

The [implementation plan](docs/IMPLEMENTATION_PLAN.md), [worklog](docs/WORKLOG.md),
and [complete upstream issue triage](docs/ISSUE_TRIAGE.md) record scope and decisions.

## Native Rust

Rust builds require Cargo/Rust and OpenSSL development headers (`libssl-dev` and
`pkg-config` on Ubuntu). They do not require Gazebo C++ libraries.

```sh
cargo build --locked --release
cargo test --locked --all-targets
cargo run --locked --bin gz-fuel -- --help
```

Add `gz-fuel-tools = { path = "/path/to/gz-fuel-tools-rs" }` to your application.
The blocking API supports details, lazy paginated listing, cache lookup, downloads,
dependencies, batches, uploads, patches, deletion and updates:

```rust,no_run
use gz_fuel_tools::{ClientConfig, FuelClient, ResourceKind};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = ClientConfig::default();
    config.cache_location = "/tmp/my-fuel-cache".into();
    let client = FuelClient::new(config);
    let id = client.parse(
        "https://fuel.gazebosim.org/1.0/OWNER/models/NAME",
        ResourceKind::Model,
    )?;
    let downloaded = client.download(ResourceKind::Model, &id, &[])?;
    println!("{}", downloaded.path.display());
    Ok(())
}
```

`ResourceId.version == 0` selects `tip`. Batch jobs are bounded to 1–64 workers;
zero from the C++ compatibility API is normalized to one. Dependency walks detect
cycles and deduplicate resources before scheduling them. Native listings implement
Rust's `Iterator<Item = Result<ResourceId, Error>>` and fetch pages lazily. Transport or decoding errors are returned once before the iterator ends. `list_collection` lists models/worlds in a collection, and `cached_resources` enumerates cached versions.

## C++ compatibility

The facade additionally requires CMake, a C++17 compiler, jsoncpp and the matching
Gazebo common library. Upstream test compilation also requires Gazebo common's
testing component, gz-msgs and the original source tree at the pinned commit.
The default test source path is the sibling `../gz-fuel-tools`; override it with
`-DUPSTREAM_SOURCE=/path/to/gz-fuel-tools`.

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=RelWithDebInfo
cmake --build build -j4
ctest --test-dir build --output-on-failure
cmake --install build --prefix /your/install/prefix
```

To build/install only the facade without an upstream test checkout, use
`-DBUILD_TESTING=OFF`. Cargo builds the Rust library as part of the CMake build.

```cmake
find_package(gz-fuel_tools REQUIRED)
target_link_libraries(my_app PRIVATE gz-fuel_tools::gz-fuel_tools)
```

Existing `#include <gz/fuel_tools/FuelClient.hh>` and other public headers retain
their class names, signatures, overloads, enum values, reference-returning methods,
and iterator operators. Rebuild downstream code against these headers. The internal
C ABI is implementation detail; it uses length-delimited buffers and byte arrays
for binary bodies. Rust panics are caught at that boundary. C++ only adapts Gazebo
URI values, containers, stable references and iterator handles.

`configure` runs deterministically without interactive prompts. `configure --console --defaults` prints the default YAML; `configure --defaults` writes it to the selected cache, and `--config` / `--cache` select locations. Existing files require `--defaults` to overwrite.

The standalone `gz-fuel` executable accepts the optional `fuel` prefix used by
`gz fuel`. CMake parity tests use a build-local `gz` launcher so they exercise this
Rust executable instead of a globally installed Fuel plugin. Installation also supplies a thin Ruby launcher for the existing `gz` plugin dispatcher; all command logic remains in Rust. The legacy exported
`gzVersion`, `downloadUrl`, `listModels` and other command hooks are also provided.

```sh
gz-fuel list -t model -o OWNER --raw
gz-fuel download -u https://fuel.gazebosim.org/1.0/OWNER/models/NAME
gz-fuel upload -m /path/to/model --header 'Private-Token: YOUR_TOKEN'
gz-fuel edit -u https://fuel.gazebosim.org/1.0/OWNER/models/NAME --public
```

Uploads read `metadata.pbtxt` or `model.config` and include all model files,
privacy, owner, tags, categories, annotations and license information. Avoid putting
real credentials in scripts committed to source control.

## Storage and intentional corrections

Models and worlds share resource parsing, HTTP and cache operations. Numeric cache
versions determine the latest resource. Downloads extract to staging directories;
failed replacement restores the previous cache entry. Cache paths preserve the
legacy URI authority behavior required by existing C++ callers.

- HTTPS verifies certificates. Resource path segments are encoded individually,
  preserving route separators and existing escapes.
- ZIPs are read/written as binary bytes. Absolute paths, traversal, drive escapes
  and symbolic links are rejected before extraction.
- Model metadata and its main SDF must be valid before a download is published.
- Referral downloads are bounded and cycle-checked; private Fuel headers are not
  forwarded to referral URLs.
- Failed requests and failed batch items propagate failure to callers and CLI exit
  status. Requested explicit versions survive a missing version response header.

Server database migrations, backend uniqueness, Python bindings and new C++ API
extensions are outside this rewrite. Native platforms other than Linux require
separate validation; keeping platform-aware code is not a claim of tested support.

## Development gates

The local repository has its tracked `.githooks/pre-commit` activated. After a new
clone run `scripts/install-hooks.sh`, or use `pre-commit install` with the included
`.pre-commit-config.yaml`. Both run formatting, Clippy with warnings denied,
production Rust coverage strictly greater than 95%, and unchanged upstream C++
parity tests. Failing tests fail the hook; network tests are not silently omitted.

```sh
rustup component add rustfmt clippy llvm-tools-preview
scripts/coverage.sh
scripts/parity.sh
scripts/cpp_coverage.sh
```

The coverage gate uses the LLVM tools shipped with the active Rust toolchain,
starts with fresh profiles and counts all production Rust files, including FFI,
CLI and error paths. Explicit `#[cfg(test)]` modules are excluded. C++ facade line
coverage is measured and gated independently above 95% with gcov, including inline bridge helpers.
The CI Rust job runs independently; the C++ job requires a `gazebo` self-hosted
runner with the matching Gazebo dependencies and pinned upstream test source.
