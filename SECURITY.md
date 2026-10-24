# Security policy

Security fixes target the current `main` branch, which implements the
12.0.0 prerelease API. Older commits and historical prerelease versions do not
have a separate security maintenance commitment. Tested platform support is
described in [README.md](README.md).

Please report suspected vulnerabilities through
[GitHub private vulnerability reporting](https://github.com/gazebosim-rs/gz-fuel-tools-rs/security/advisories/new).
Include the affected commit/version, environment, reproduction steps, expected
security boundary, and impact. Share a minimal reproducer without live tokens,
private Fuel assets, or personal data. Keep exploit details out of public issues
and pull requests until coordinated disclosure.

Relevant boundaries include untrusted URLs and server responses, archive
extraction and cache replacement, credential forwarding, and the Rust/C++ FFI.
Maintainers assess reports, coordinate fixes and disclosure with the reporter,
and publish advisories when appropriate. This project does not promise a fixed
response time.

Dependency advisories, licenses, and sources are checked with `cargo-deny` using
[deny.toml](deny.toml). An audit passing is evidence about the database and graph
checked at that time, not a guarantee that no vulnerabilities exist. Exceptions
must be narrow, justified, and assigned a review deadline.
