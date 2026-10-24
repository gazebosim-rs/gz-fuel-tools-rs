# Changelog

## Unreleased

- Keep upload metadata literal and scope dependency download credentials to their
  intended origin; cover both behaviors with local regression servers.
- Traverse cached dependencies to fetch missing descendants, and report listing
  failures instead of claiming a successful empty collection download.
- Replace unmaintained YAML dependencies and upgrade ZIP handling to versions
  passing the current advisory audit without exceptions.
- Add dependency, contributor and release preparation policies and checks.
- Upgrade ZIP handling to the maintained 9.x release and replace deprecated YAML
  parsing with serde-saphyr, removing known dependency audit findings.
- Validate crate packaging and a clean source archive's C++ build, installation,
  relocation, CMake and pkg-config consumers before release.

## 12.0.0-pre.2 — candidate, not published

- Implement the native Rust Fuel client and CLI, with a source-compatible C++
  facade for upstream gz-fuel-tools 12.0.0-pre2 at commit
  `6ac62b01d7d8af3ab23fd2239728a791704a3f09`.
- Retain upstream public declarations and test behavior; add safer archive
  extraction, bounded dependency downloads and error propagation.
- Validate Linux builds with independent Rust and C++ production coverage above
  95%; see `docs/VERIFICATION.md` for the measured baseline and limitations.
- Require downstream C++ code to rebuild. Binary interchangeability with the
  original library and support for untested platforms are not claimed.
