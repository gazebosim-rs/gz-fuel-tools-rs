# Upstream issue review

Reviewed the live GitHub REST API on 2026-10-24 as part of the initial audit. API returned 45 open items: 44 issues and one pull request (#461). This is a triage, not a claim that every candidate is implemented or the upstream issue can be closed. Historical open issues can describe behavior already changed on the checked-out 12.0.0-pre2 tip.

| Issue | Assessment |
| --- | --- |
| [#468: Python bindings for fuel tools](https://github.com/gazebosim/gz-fuel-tools/issues/468) | Outside scope: Python bindings are a separate language binding project. |
| [#461: Add a flag --skip-cached to skip already-downloaded models and collections](https://github.com/gazebosim/gz-fuel-tools/pull/461) | Open PR proposing cache skip; overlaps #150. Review semantics, do not treat as an open issue. |
| [#454: Model files are modified (PNGs are corrupt) after extraction from downloaded zip file (Windows, conda, gz-fuel-tools10-10.0.1)](https://github.com/gazebosim/gz-fuel-tools/issues/454) | Fix: binary-safe ZIP extraction; byte-for-byte binary regression. |
| [#431: Enable codecov for Harmonic and Ionic branches](https://github.com/gazebosim/gz-fuel-tools/issues/431) | Fix in rewrite: measured coverage gate and CI; upstream branch enablement is external. |
| [#424: Regression: Resource spawner in gz-sim fails after #376](https://github.com/gazebosim/gz-fuel-tools/issues/424) | Preserve model listing/iterator URL identities and verify downstream API parity; requires gz-sim integration to claim upstream closure. |
| [#420: Gz fuel server allows models with duplicate author and name](https://github.com/gazebosim/gz-fuel-tools/issues/420) | Outside client scope: backend uniqueness constraint; client must avoid silently overwriting distinct IDs. |
| [#254: Tests that use staging-fuel.ignitionrobotics.org are failing](https://github.com/gazebosim/gz-fuel-tools/issues/254) | Fix test reliability: deterministic local REST fixtures, retaining original network tests separately. |
| [#216: Redirect app.ignitionrobotics.org URLs to corresponding fuel.ignitionrobotics.org resources](https://github.com/gazebosim/gz-fuel-tools/issues/216) | Candidate: map known web UI URLs to Fuel API; preserve explicit server URLs and avoid speculative host rewriting. |
| [#214: redirectIO helper function in ign_src_TEST.cc doesn't work on all platforms](https://github.com/gazebosim/gz-fuel-tools/issues/214) | Fix tests: capture at C++ std::cout/std::cerr boundary and verify both streams. |
| [#202: stderr ignition-fuel_tools6 while building](https://github.com/gazebosim/gz-fuel-tools/issues/202) | Historical packaging/build issue: use Cargo/CMake dependency checks; old ignition release is outside scope. |
| [#194: Make all models from Gazebo's database work well on Ignition](https://github.com/gazebosim/gz-fuel-tools/issues/194) | Outside scope: model database content migration. |
| [#186: Document how to upload a model from the command line](https://github.com/gazebosim/gz-fuel-tools/issues/186) | Fix documentation: upload example with token header and metadata. |
| [#177: Add status hooks/callback to FuelClient](https://github.com/gazebosim/gz-fuel-tools/issues/177) | Defer API expansion: preserve existing signatures; native Rust API may grow progress callbacks later. |
| [#168: The `--owner` flag in `ign fuel list` switches the command to search only cached models](https://github.com/gazebosim/gz-fuel-tools/issues/168) | Fix: owner-filtered server queries must be attempted rather than forcing cache-only. |
| [#150: Add option to skip cached models when downloading from command line](https://github.com/gazebosim/gz-fuel-tools/issues/150) | Candidate: opt-in cache skip; current tip may already cover some paths; preserve download signatures. |
| [#120: Ability to download a model by model name ](https://github.com/gazebosim/gz-fuel-tools/issues/120) | Defer ambiguous convenience API: name alone is not a globally unique identifier. |
| [#113: Command line tools failing on Windows and macOS](https://github.com/gazebosim/gz-fuel-tools/issues/113) | Fix portability by Rust CLI implementation; actual Windows/macOS CI required to claim platform support. |
| [#99: Cache resource list](https://github.com/gazebosim/gz-fuel-tools/issues/99) | Defer persistent index until invalidation semantics defined; existing cache enumeration preserved. |
| [#93: cant build ign-fuel-tools1 on windows: %win_dir% is empty, cant found YAML (yaml-0.1), unknown include path appeared, failed to nmake, etc.](https://github.com/gazebosim/gz-fuel-tools/issues/93) | Historical ignition1 packaging issue superseded by Cargo dependencies; native platform CI still needed. |
| [#80: Fails to resolve model paths on case-insensitive filesystems like in macOS](https://github.com/gazebosim/gz-fuel-tools/issues/80) | Fix comparisons/canonical cache identities consistently; do not silently merge unequal remote resources. |
| [#51: Use ign-math SemanticVersion in LocalCache.cc](https://github.com/gazebosim/gz-fuel-tools/issues/51) | Use typed numeric resource versions rather than directory lexical ordering; semantic-version crate unnecessary for API numeric versions. |
| [#50: Consider checking for the existence of files at time of download](https://github.com/gazebosim/gz-fuel-tools/issues/50) | Fix: fetch/check cached paths and avoid treating incomplete extraction as complete. |
| [#49: Unversioned file share/ignition/fuel_tools/config.yaml](https://github.com/gazebosim/gz-fuel-tools/issues/49) | Fix: install versioned config path and retain documented user override precedence. |
| [#48: 4 unit tests fail: UNIT_FuelClient_TEST, UNIT_ign_src_TEST, UNIT_ModelIdentifier_TEST, UNIT_ign_TEST](https://github.com/gazebosim/gz-fuel-tools/issues/48) | Historical ignition1 failures: run complete current test suite; do not reuse old expected outputs. |
| [#47: warning: moving a temporary object prevents copy elision](https://github.com/gazebosim/gz-fuel-tools/issues/47) | Fix: C++ facade uses ordinary return/move semantics without pessimizing moves. |
| [#46: FuelClient API expects redundant possibly conflicting information](https://github.com/gazebosim/gz-fuel-tools/issues/46) | Fix internal design: one normalized identifier carries server; retain compatibility overloads. |
| [#42: FuelClient support range-based for-loops](https://github.com/gazebosim/gz-fuel-tools/issues/42) | Native Rust iterators supported conceptually; C++ API expansion deferred because preserving headers is primary requirement. |
| [#41: ignition-fuel_tools1 Requires.private: libcurl >=  jsoncpp](https://github.com/gazebosim/gz-fuel-tools/issues/41) | Fix generated pkg-config dependency syntax; historical ignition1 package external. |
| [#34: [Fuel backend] Extend REST class to support multi-part form-data POST](https://github.com/gazebosim/gz-fuel-tools/issues/34) | Preserve supported multipart POST through Rust HTTP transport and test local requests. |
| [#32: [Fuel backend] Support World CRUD operations](https://github.com/gazebosim/gz-fuel-tools/issues/32) | Preserve current world fetch/list/delete/update operations; world upload API expansion deferred. |
| [#31: [Fuel backend] Support updating a model](https://github.com/gazebosim/gz-fuel-tools/issues/31) | Preserve model PATCH/update and CLI edit with tested multipart requests. |
| [#28: [Fuel backend] Update test-server with current API](https://github.com/gazebosim/gz-fuel-tools/issues/28) | Fix: deterministic local current-API server fixtures. |
| [#26: [Fuel backend] ModelIdentifier::UniqueName() shouldn't allow slashes between url and owner](https://github.com/gazebosim/gz-fuel-tools/issues/26) | Fix: normalize separator joining without altering scheme delimiter. |
| [#25: [Fuel backend] ModelIdentifierPrivate::ValidName isn't implemented](https://github.com/gazebosim/gz-fuel-tools/issues/25) | Fix: validate parsed URL and path components; reject traversal and malformed authorities. |
| [#24: [Fuel backend] Some stuff on ModelIdentifier should be on Model instead](https://github.com/gazebosim/gz-fuel-tools/issues/24) | Internal Rust representation may separate identity/metadata; retain all public C++ accessors. |
| [#23: [Fuel backend] LocalCache should return a vector of models rather than a ModelIter](https://github.com/gazebosim/gz-fuel-tools/issues/23) | Fix internal design: Rust cache returns explicit collections; facade adapts legacy iterator. |
| [#21: [Fuel backend] Support pagination](https://github.com/gazebosim/gz-fuel-tools/issues/21) | Preserve/fix lazy pagination for both model/world lists; terminate on empty/error page. |
| [#20: [Fuel backend] List models from multiple servers](https://github.com/gazebosim/gz-fuel-tools/issues/20) | Candidate: list across configured servers in CLI; preserve single-server overload behavior. |
| [#18: [Fuel backend] Model class needs a factory](https://github.com/gazebosim/gz-fuel-tools/issues/18) | Simplify using explicit Rust values and small facade factories; no extra C++ factory hierarchy. |
| [#17: [Fuel backend] Mock LocalCache and REST](https://github.com/gazebosim/gz-fuel-tools/issues/17) | Fix: deterministic local HTTP and temporary cache tests, including error paths. |
| [#16: [Fuel backend] JSONParser::BuildModel() doesn't match current backend](https://github.com/gazebosim/gz-fuel-tools/issues/16) | Preserve current backend JSON contracts with fixture tests. |
| [#11: [Fuel backend] FuelClient cannot list models by owner or name from REST](https://github.com/gazebosim/gz-fuel-tools/issues/11) | Fix: owner/name-filtered query routes and result filtering. |
| [#10: [Fuel backend] ign fuel list models from local cache](https://github.com/gazebosim/gz-fuel-tools/issues/10) | Defer new CLI flag; native cache enumeration exists and existing fallback preserved. |
| [#8: Support defining access rights](https://github.com/gazebosim/gz-fuel-tools/issues/8) | Preserve private/public upload/edit flags. |
| [#7: Specify tags via command line](https://github.com/gazebosim/gz-fuel-tools/issues/7) | Preserve tags from metadata; new command-line flags outside compatibility scope. |

## Archive/cache/transport requirements

- ZIP extraction must preserve bytes, reject absolute paths, parent traversal, drive/UNC escapes and links; partial failures must not publish a complete cache entry.
- Cache paths must remain beneath configured roots, resolve numeric versions correctly, handle failed writes, and distinguish existing complete resources from partial directories.
- Transport must retain TLS verification, encode components without encoding routing separators, propagate HTTP failures, preserve multipart duplicate keys, and avoid leaking tokens to referral hosts.
- Regression fixtures should cover binary PNG-like bytes, malicious archives, mixed case identifiers, nested files, pagination, server failure, owner filtering, and failed command exit statuses.
