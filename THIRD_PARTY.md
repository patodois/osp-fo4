# Third-party attribution

## Material Editor / MaterialLib

`fo4-material` is an attributed Rust port of the binary layout in
[ousnius/Material-Editor](https://github.com/ousnius/Material-Editor),
`MaterialLib/BaseMaterialFile.cs`, `BGSM.cs`, and `BGEM.cs`, at commit
`21411873f17454a2442d6785d533499e14c63adb`.
The following MIT notice applies to that attributed material. The Rust bounds,
borrowing, diagnostics, IEEE preservation, tests and VFS integration are local work.

```text
The MIT License

Copyright (c) 2018 ousnius

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

## Ruffle's `swf` crate

`fo4-ui` depends on the `swf` crate, version 0.4.0, published on crates.io by
the Ruffle project (<https://ruffle.rs>, repository
<https://github.com/ruffle-rs/ruffle>), licensed MIT OR Apache-2.0. It parses
the interface movies' tags, shapes, texts, fonts, sprites and the AVM2 (ABC)
blocks; `fo4-ui` calls it as a dependency with its default features off (no
zlib or LZMA of its own: compressed bodies are inflated by `fo4_core::zlib`
under the reader's caps). No Ruffle source is copied into this repository;
Ruffle's public documentation is the only other Ruffle material consulted.
Its transitive packages are listed in the table below with the others.

## The viewer's thread clock

The GPU viewer (`apps/fo4-view`, its own workspace) depends directly, on
Unix targets, on the `rustix` crate, version 1.1.6 as locked, published on
crates.io by the Bytecode Alliance project, licensed Apache-2.0 WITH
LLVM-exception OR Apache-2.0 OR MIT. It reads the play data thread's CPU
clock (`clock_gettime` with the thread CPU-time clock) through its safe
interface, with its default features off and `std` and `time` on. It was
already in the viewer's lock through the window crates; no source of it is
copied into this repository.

## The viewer's pad

The GPU viewer (`apps/fo4-view`, its own workspace) depends, only with its
optional feature `pad` (off by default; `--play-pad`), on the `gip-pad`
crate, version 0.4.0, published on crates.io by the repository's owner
(repository <https://github.com/patodois/gip-pad>), licensed MIT OR
Apache-2.0. It reads a GIP pad over USB on its own thread through its
public interface (`PadReader::spawn`, `PadReader::spawn_without_reset`,
`latest`, `last_error`, `stick`, `Pad`'s fields and the `BUTTONS` table, by
name); only its published documentation was read, and no source of it is
copied into this repository. With it, these packages are new in the
viewer's lock (versions and declared licenses from offline locked Cargo
metadata):

| Package | Version | Declared license |
| --- | --- | --- |
| anyhow | 1.0.105 | MIT OR Apache-2.0 |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 |
| gip-pad | 0.4.0 | MIT OR Apache-2.0 |
| io-kit-sys | 0.5.0 | MIT OR Apache-2.0 |
| mach2 | 0.5.0 | BSD-2-Clause OR MIT OR Apache-2.0 |
| nusb | 0.2.9 | Apache-2.0 OR MIT |

The rest of its dependency tree was already in the viewer's lock through
the window crates, every one under a permissive license (MIT, Apache-2.0,
Zlib or BSD terms, as their metadata declares): bitflags, bumpalo,
bytemuck, cfg-if, core-foundation-sys, errno, futures-core, futures-macro,
futures-task, futures-util, libc, log, once_cell, pin-project-lite,
proc-macro2, quote, rustix, serde_core, serde_derive, slab, syn, tokio,
unicode-ident and, for other
targets, js-sys, linux-raw-sys, wasm-bindgen and its macro crates, web-sys,
windows-link and windows-sys.

## Cargo dependencies

Versions and declared licenses from offline locked Cargo metadata. The first-party
crates use MIT OR Apache-2.0. Dependencies are obtained from crates.io; no cloned
third-party code is executed. No new dependency was added for image diagnostics
or reference rendering.

| Package | Version | Declared license |
| --- | --- | --- |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| autocfg | 1.5.1 | Apache-2.0 OR MIT |
| bcn-decode | 0.1.0 | MIT OR Apache-2.0 |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| bitstream-io | 4.10.0 | MIT/Apache-2.0 |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| core_detect | 1.0.0 | MIT/Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 |
| crc32fast | 1.5.4 | MIT OR Apache-2.0 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 |
| deranged | 0.5.9 | MIT OR Apache-2.0 |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| encoding_rs | 0.8.44 | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| enum-map | 2.7.3 | MIT OR Apache-2.0 |
| enum-map-derive | 0.17.0 | MIT OR Apache-2.0 |
| flate2 | 1.1.11 | MIT OR Apache-2.0 |
| generic-array | 0.14.7 | MIT |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| libc | 0.2.191 | MIT OR Apache-2.0 |
| log | 0.4.35 | MIT OR Apache-2.0 |
| lz4_flex | 0.14.0 | MIT |
| memchr | 2.8.3 | Unlicense OR MIT |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| multiversion_no_op | 1.0.0 | Apache-2.0 OR MIT |
| no_std_io2 | 0.9.4 | Apache-2.0 OR MIT |
| num-bigint | 0.4.8 | MIT OR Apache-2.0 |
| num-conv | 0.2.2 | MIT OR Apache-2.0 |
| num-derive | 0.4.2 | MIT OR Apache-2.0 |
| num-integer | 0.1.47 | MIT OR Apache-2.0 |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| powerfmt | 0.2.1 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| rustversion | 1.0.23 | MIT OR Apache-2.0 |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| simd-adler32 | 0.3.10 | MIT |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 |
| simple_asn1 | 0.6.4 | ISC |
| swf | 0.4.0 | MIT OR Apache-2.0 |
| syn | 2.0.119 | MIT OR Apache-2.0 |
| syn | 3.0.9 | MIT OR Apache-2.0 |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |
| time | 0.3.55 | MIT OR Apache-2.0 |
| time-core | 0.1.9 | MIT OR Apache-2.0 |
| time-macros | 0.2.32 | MIT OR Apache-2.0 |
| twox-hash | 2.1.5 | MIT |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| version_check | 0.9.5 | MIT/Apache-2.0 |
| zmij | 1.0.23 | MIT |

`bcn-decode` supplies BC6H/BC7 decoding; the other texture layouts and DDS framing
are local. `flate2` uses its Rust backend, and `lz4_flex` handles bounded LZ4
blocks/frames. `serde`/`serde_json` serialize explicit reports; `sha2` and
`twox-hash` implement content/checksum hashing. Transitive codec, hash and macro
packages are listed above. Neither the `ba2` crate nor DirectXTex is used.
