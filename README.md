# OSP FO4

Rust readers and a CPU reference renderer for people who own Fallout 4.
The project reads assets from your own installation and produces inventories,
structured diagnostics and unlit images. It does not run the game.

The workspace covers BA2/BSA archives, plugin envelopes and selected record
semantics, localized strings, a NIF subset, BGSM/BGEM materials, DDS textures,
explicit virtual file resolution and static scene assembly. Readers preserve
unknown bytes and report unsupported cases with their source and offset.

A measured seven-plugin installation contains 39,188 inspected bases: 17 pass
strictly (16 light declarations and one empty collection), 25,341 require
registered hypotheses, and 13,830 remain blocked. No mesh placement in that
census passes without hypotheses.
The Financial13 reference scene draws 293 of 339 geometric placements;
45 REFR placements and one excluded NAVM do not draw. These counts describe
that corpus, not compatibility with every installation. See the
[spec index and hypothesis register](spec/README.md).

There is no game lighting, full shader reproduction, animation/controller
execution, particles, gameplay or general exterior renderer. Rotation order is
supported by measured matrices; its exact floating-point arithmetic is still
open. The NIF/runtime local-matrix bridge is also under comparison. MODT, material blend equivalence, SCOL geometry, effect colors, backface
culling and named helper visibility retain explicit limitations. The renderer
is a diagnostic reference, not a pixel-exact recreation of the game.

Use Rust 1.88 or newer. Set `FO4_DATA` to the **Data directory** of your own copy,
then run this command from the repository:

```sh
export FO4_DATA="/path/to/Fallout 4/Data"
CARGO_HOME="$PWD/.cargo-home" CARGO_TARGET_DIR="$PWD/target" CARGO_INCREMENTAL=0 cargo run --release -j 4 -p fo4-unpack --example render -- examples/fallout4.json "$FO4_DATA" Financial13 build/example --under-hypotheses
```

It writes `build/example/frame.png` and `summary.json`. Choose a new output
directory for each run. The camera is an authored overview of the scene bounds;
measured-camera comparisons use separately collected camera data. The example
manifest explicitly lists the base-game plugin and archives. Adjust that file
for your installed archive set; load order and mount priority are never inferred
from directory order or timestamps. `--under-hypotheses` permits the recorded
candidate rules and lists them in the output; `--strict` keeps them blocked.
No game files, derived textures, photographs or measurement datasets are bundled.

`fo4-unpack` also provides `inventory`, `resolve`, `sweep` and `check-cell`.
Their explicit manifest and report contracts are documented in
[manifest-vfs.md](spec/manifest-vfs.md) and [scene.md](spec/scene.md).

Default tests build their own synthetic bytes and require neither the game nor
network access once dependencies are cached:

```sh
export CARGO_HOME="$PWD/.cargo-home" CARGO_TARGET_DIR="$PWD/target" CARGO_INCREMENTAL=0 RUST_TEST_THREADS=4
cargo test --workspace --offline -j 4
cargo fmt --check
cargo clippy --workspace --all-targets --offline -j 4 -- -D warnings
```

Two synthetic index scale measurements are ignored by default for runtime cost.
They need no game data. The optional `measured_transforms` replay is also explicitly ignored. To run it,
supply your compatible measurement fixture through `FO4_TRANSFORM_FIXTURE` and
use `cargo test -p fo4-unpack --test measured_transforms -- --ignored --nocapture`.
Its exact comparisons cover only that fixture; they do not establish a universal
rotation arithmetic law.

Format facts come from cited public documentation and independently recorded
measurements of a running, owned copy. Candidate behavior is registered before
use, compared with measured transforms or repeated photographs, and kept
separate from confirmed field layouts. No decompiled game code is used.
[Provenance and source boundaries](docs/provenance.md) explain the existing
attributed MaterialLib layout port and third-party dependencies.

Project code is licensed under [MIT](LICENSE-MIT) OR
[Apache-2.0](LICENSE-APACHE), with the notices in [THIRD_PARTY.md](THIRD_PARTY.md).
AI coding assistance was used extensively; generated changes are reviewed through
bounded readers, synthetic corruption tests and explicit measurement comparisons.
