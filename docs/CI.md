# Pull request CI and merge policy

Every pull request runs the `CI` workflow on GitHub-hosted Ubuntu 24.04 runners.
There are no path filters or draft-PR skips. Pushes to main, merge groups and manual
workflow dispatches run the same checks. The full workflow also runs weekly so
new dependency advisories can be detected without a new pull request.

- **Rust checks**: Rust 1.92 formatting, all-target Clippy with warnings denied,
  all unit/integration tests, and production Rust line coverage strictly above 95%.
  Documentation is built with warnings denied, missing public API documentation
  is rejected, and doctests run separately. The embedded upstream protobuf schema
  and parser corpus are checked against protoc and the built Rust CLI.
- **C++ compatibility**: builds the pinned Gazebo dependencies on a hosted runner,
  compiles unchanged upstream tests plus supplementary compatibility tests, runs
  all 22 suites (including inherited disabled service cases), and requires C++
  facade line coverage strictly above 95%. It also verifies the crate package and
  builds a clean source archive, installs and relocates the library, and compiles
  external CMake and pkg-config consumers. Candidate packages and checksums are
  retained as CI artifacts; nothing is published. See [RELEASING.md](../RELEASING.md).
- **Dependency and workflow policy**: validates workflow syntax and embedded shell
  with checksum-pinned actionlint and ShellCheck, then uses pinned cargo-deny to
  check the locked graph against current advisories, license and source policy.
- **CI required**: runs even when a dependency fails or is skipped. It succeeds only
  when every required job succeeds. This is the required merge check.

Dependencies are pinned in `scripts/ci/dependencies.lock`. The installed libraries
are cached by OS, architecture and the hashes of that lock and the build script.
Dependency tests and unrelated Gazebo components are omitted; Fuel's own complete
parity suite remains enabled. No self-hosted runner is needed. Coverage and CTest
logs are retained as workflow artifacts, including on failures.

Actions are pinned to full commit SHAs; Dependabot proposes weekly action and
Cargo updates through ordinary protected PRs. The workflow grants only
contents-read token permissions. PR jobs use ephemeral
GitHub-hosted runners and do not receive a repository write credential through
checkout. Superseded runs for the same PR are canceled.

GitHub vulnerability alerts, Dependabot security fix PRs, secret scanning, secret
push protection and private vulnerability reporting are enabled. These live
settings complement the tracked [security policy](../SECURITY.md) and
[dependency policy](../deny.toml); they do not replace review or tests.

## Main branch enforcement

The live GitHub branch protection settings are represented by
`.github/branch-protection.json`:

- Changes must go through a pull request.
- `CI required` must succeed for the latest commit, with the branch up to date with
  main. The check is bound to the GitHub Actions application (app ID 15368).
- Rules apply to administrators; no bypass actor is configured.
- Force pushes and branch deletion are disabled.
- No additional human approval count is imposed by this test-gating policy.

Renaming the required job requires updating branch protection at the same time.
Maintainers with repository administration access can reapply the saved policy:

```sh
gh api --method PUT \
  repos/gazebosim-rs/gz-fuel-tools-rs/branches/main/protection \
  --input .github/branch-protection.json
```

The upstream integration tests contact public Fuel services. A service failure
fails CI and blocks merging; it is not treated as a pass or silently skipped.
