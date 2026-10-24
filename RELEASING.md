# Release preparation and publication

The current version is a prerelease. Passing CI or preparing artifacts does not
authorize a tag, GitHub release, crates.io publication, or downstream package upload.
Those actions require explicit maintainer authorization for the exact version and
commit. No workflow publishes automatically and no publishing credential is needed
for the checks below.

## Prepare a candidate

1. Make release preparation changes in a pull request. Update `CHANGELOG.md` with
   user-visible changes, compatibility limits and known issues. Keep the Cargo
   package version, lockfile, CMake base/full versions, shared-library ABI major,
   CLI and exported `gzVersion()` versions, user agents and versioned dispatcher
   filenames consistent.
   `python3 scripts/check_versions.py` checks their agreement. Rust uses
   `12.0.0-pre.2`; the corresponding Gazebo compatibility string is `12.0.0~pre2`.
   A major ABI change also requires reviewing installed headers and package names.
2. Run the contributor checks and required CI jobs, including unchanged upstream
   parity tests and both coverage gates above 95%. Preserve the pinned upstream
   revision and dependency revisions in the validation record. Current platform
   support is verified Linux; do not advertise macOS or Windows without evidence.
3. From a clean checkout of the candidate commit, run
   `scripts/ci/release-checks.sh`. It checks versions, verifies the Cargo package,
   builds the C++ facade from a Git source archive with `BUILD_TESTING=OFF`, installs
   and relocates it, then compiles and runs separate CMake and pkg-config consumers
   against the installed library. It also checks the installed CLI version.
   It loads the installed gz-tools YAML manifest and executes its Ruby launcher
   before moving the prefix. After the relocation checks, it reinstalls at the
   new prefix and repeats that manifest-driven check.
   It requires Python 3.11+, Ruby, Git, Cargo, CMake, a C++17 compiler, OpenSSL, jsoncpp,
   pkg-config and the matching Gazebo common installation. The existing CI
   dependency prefix can be supplied through `CMAKE_PREFIX_PATH`,
   `PKG_CONFIG_PATH` and `LD_LIBRARY_PATH`.
   The library, standalone CLI and package metadata are relocatable. gz-tools
   requires an absolute `library_path` in its dispatcher YAML; moving the prefix
   requires `cmake --install <build-directory> --prefix <new-prefix>` to refresh
   that manifest before `gz fuel` can use the new location.
4. Review the candidate files in `build/release-checks/`: the full source archive,
   Rust crate, SHA-256 checksums and exact Git commit. `FUEL_BUILD_JOBS` controls
   parallel compilation; `FUEL_RELEASE_LIBDIR` defaults to `lib` and accepts
   `lib64` or the platform's relative multiarch directory. Use a distinct
   `FUEL_RELEASE_OUTPUT_DIR` when validating multiple layouts. Additional layout
   runs belong in validation evidence; they are not claims of another OS's support.
5. Review package contents with `cargo package --list --locked`. The crate is the
   native Rust distribution: source, Rust CLI tests, license and documentation.
   The Git source archive additionally contains the C++ facade, headers, CMake
   integration and developer tooling. Build C++ from that archive with testing
   disabled, or supply the separately pinned upstream test checkout for parity
   testing. Neither distribution vendors third-party dependencies.
   Keep include patterns anchored to the package root. The release script checks
   both Cargo's list and the generated archive against the native distribution's
   allowed paths, preventing files from ignored nested checkouts from entering it.
6. Run `cargo publish --dry-run --locked` with registry access before requesting
   publication. The dry run uploads nothing, but Cargo may contact crates.io; an
   offline failure does not count as a successful dry run. Confirm that the crate
   name and version are available and that the publishing account has ownership.
7. Merge through the protected branch, then confirm that the exact main commit
   has passed all required checks. Re-run candidate preparation at that commit;
   a merge commit can differ from the previously reviewed PR commit. Record the
   commit, workflow evidence, checksums, toolchains and dependency revisions in
   the proposed release notes. Retain the Apache license and upstream notices.
   If compiled artifacts are proposed for distribution, collect and retain all
   applicable third-party license and data notices from the exact locked graph
   and native dependencies, including encoding_rs's BSD-3-Clause and WHATWG data
   notices. A passing license allowlist audit does not generate these notices.

## Publish only after authorization

Present the exact commit, proposed `v<Cargo version>` tag, release notes, candidate
artifacts and publishing destinations for approval. After explicit authorization,
an authorized maintainer may create/push that tag, create the GitHub release with
the reviewed source archive and checksums, and run `cargo publish --locked` from
the same clean checkout if crates.io publication was included in the approval.
Use the registry's supported credential mechanism; never commit a token or print
it in logs. Do not add a token to ordinary PR CI.

Confirm the published crate/version and released artifact checksums against the
reviewed candidate. Cargo publication permanently reserves a version: corrections
require another version; yanking restricts resolution but does not erase it.
Document any failed or partial publication before attempting a retry. Review the
specific destination's state first so an interrupted command does not lead to
duplicate or mismatched releases.
