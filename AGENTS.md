# Repository guidance

Read [README.md](README.md), [CONTRIBUTING.md](CONTRIBUTING.md), and
[docs/CI.md](docs/CI.md) before changing this repository. User instructions and
confirmed owner decisions take precedence over defaults in these documents.

- Preserve the native Rust implementation and source-compatible C++ facade.
  Do not link the original Fuel implementation or alter retained upstream public
  headers/test semantics to hide incompatibilities.
- Keep tasks focused. Inspect existing changes and coordinate ownership before
  editing shared files; preserve work outside your assignment.
- Implement meaningful regression coverage for changed behavior and failure
  paths. Keep production Rust and C++ coverage strictly above 95%. Never weaken
  gates, silently skip failing tests, or describe unrun checks as passing.
- Keep unsafe and FFI code narrow. Explain soundness, ownership, lifetime,
  nullability, bounds, and threading. Keep panics/exceptions inside their language
  boundary. Review security-sensitive URL, archive, cache, and credential behavior.
- Use the declared Rust minimum and locked dependency graph. Follow the audit,
  source, license, action-pinning, and exception rules in CONTRIBUTING.md.
- Request independent review for substantial security, unsafe/FFI, or
  compatibility work. Use review agents when explicitly requested or assigned;
  do not invent an additional mandatory human approval policy.
- Run checks appropriate to the change and the required merge gates. Report the
  exact validation and unresolved limitations; keep verification evidence current
  when behavior or measured results change. Do not fabricate coverage or claims
  of support for platforms not tested.
- Keep secrets out of code, logs, fixtures, and generated artifacts. Follow
  SECURITY.md for vulnerability reporting.
- Ordinary development authorizes preparation and validation, not real package
  publication, release tagging, or destructive operations. Obtain explicit scope
  for those actions and for sending messages to outside people or services.
