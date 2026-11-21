# Provenance and method

The archive, plugin, NIF, VFS, scene and CPU renderer code is original project
work. Public documentation and schemas establish binary facts; each spec records
its sources and the fields they support. No decompiled game code, game executable,
redistributed game asset or third-party game-reader implementation is a source.
Permitted schemas include nifxml; documentation includes format references,
UESP/modding documentation and independently checked field/byte measurements.
An external tool's printed data is evidence for the observed fields, not permission
to read or translate that tool's code.

`fo4-material` contains an explicitly attributed Rust port of MaterialLib's
BGSM/BGEM layouts at the exact commit recorded in THIRD_PARTY.md. This existing
MIT-licensed exception predates the current source restrictions. Its license
notice remains intact. No additional Material Editor implementation was consulted
for this work. The bounds, diagnostics, borrowing, IEEE preservation and synthetic
tests are local work.

The Fallout 4 collision readers use only the nifxml definitions of
`NiCollisionObject`, `bhkCOFlags`, `bhkNPCollisionObject`, `bhkSystem`,
`bhkPhysicsSystem`, `bhkRagdollSystem` and `ByteArray` (same pinned copy as
spec/nif.md). The embedded Havok payload is described only by counts measured
over the installed game ([spec/collision.md](../spec/collision.md)); no Havok
SDK, header, converter or other reader of that format was consulted.

Runtime experiments measure an owned running copy through separately collected
camera/transform words and repeated photographs. Their raw data stays local.
Behavior not established by a format source is registered in
[the hypothesis table](../spec/hypotheses.tsv) before use, with a falsifiable
measurement and an explicit status. Support for a convention does not establish
bit-exact arithmetic, a whole renderer, or unmeasured record types. Stable values
in two pauses do not prove absence of animation or authorize controller execution.

All default tests generate their own bytes. The optional transform fixture replay
is ignored unless explicitly requested; its input is not distributed. Real
installations are read-only. Images, extracted fragments, measurements and local
reports belong in ignored output folders and are never part of source artifacts.

Cargo.lock pins the dependency graph. THIRD_PARTY.md lists the package versions
and declared SPDX licenses verified against offline Cargo metadata. This is a
source and notice inventory, not a claim that unknown format behavior has been
established. AI assistance was used for implementation and documentation; test
results, limitations and measurement scope remain explicit.
