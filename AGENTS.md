# Contributor and agent guide

Read [README.md](README.md) first for what the project is and how to run it.

## Map

| Crate | Purpose | Start here |
| --- | --- | --- |
| `fo4-core` | Checked I/O (`ReadAt`, `Cursor`), allocation `Budget`, `Error`, `Manifest`. | `src/lib.rs` |
| `fo4-archive` | BA2 and BSA indexes and bounded extraction. | `src/lib.rs`, `src/bsa.rs` |
| `fo4-vfs` | Explicit mounts, winner and loser per path, content fingerprint. | `src/lib.rs` |
| `fo4-plugin` | Records, load order, overrides, strings, typed views, subrecord layouts. | `src/lib.rs`, `src/typed.rs`, `src/layout.rs` |
| `fo4-nif`, `fo4-material`, `fo4-texture` | Model, material and texture readers. | `src/lib.rs` of each |
| `fo4-scene` | Cell loading, asset graph, rule levels and the rule registry. | `src/lib.rs`, `src/hypotheses.rs` |
| `fo4-render` | Draw list, CPU rasterizer, PNG, image comparison. | `src/draw.rs`, `src/raster.rs` |
| `fo4-unpack` | Command-line reports and the `render` example. | `src/lib.rs`, `examples/render.rs` |

Format layouts, their sources and the rule registry are indexed in
[spec/README.md](spec/README.md).

## Rules of evidence

- A **format fact** needs a public source: cite the URL and its exact scope in
  the relevant spec file and in [docs/provenance.md](docs/provenance.md).
- Any **behaviour** not settled by a source is a named rule in
  [spec/hypotheses.tsv](spec/hypotheses.tsv). Register it as `pending`, with the
  measurement that could refute it, before code relies on it. Only a recorded
  measurement of the running game changes its status. The code reads the
  registry: see `fo4_scene::RuleLevel` and `fo4_scene::hypotheses`.
- Never turn an unsupported assumption into a default. Preserve unknown bytes,
  or return a named error with file, offset, expected and found.
- Keep a measured convention separate from claims about exact floating-point
  arithmetic.

## Sources that are off limits

Do not open, translate or paraphrase third-party implementations of these
formats (for example nifly, NifSkope, PyNifly, BodySlide, fo76utils, OpenMW,
xEdit source or scripts, Mutagen, Material Editor), and never inspect game
executables or decompiled code. Public format documentation and schemas such as
nifxml are allowed. The one attributed exception, a port of the MaterialLib
layout, is documented in [THIRD_PARTY.md](THIRD_PARTY.md) and is not permission
to consult further implementation code.

## What must never be committed

Game assets, images containing game textures, photographs of the game,
executable dumps, measurement datasets, local machine paths and secrets. Put
local artifacts under `build/` or `reports/`, which Git ignores. A test
(`fo4-unpack/tests/repository_guard.rs`) rejects game file types and machine
paths in versioned files.

## Code conventions

- `#![forbid(unsafe_code)]` everywhere.
- Checked arithmetic (`fo4_core::add`, `mul`, `range`, `limit`), `try_reserve`
  through a `Budget`, and bounded counts, depths and decompressed sizes for
  anything read from a file.
- Load order and archive priority come only from the manifest.
- Tests use synthetic bytes and must pass without the game. A test that needs
  real data is `#[ignore]` with the reason in the attribute.
- Game installations are read-only: tools write only inside their output
  directory.

## Checks before a change is done

```sh
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

If the change could alter output, render a cell before and after and compare
the files byte for byte; say so in the commit message when a difference is
intended.
