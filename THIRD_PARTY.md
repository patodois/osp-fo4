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

## Cargo dependencies

Versions and declared licenses from offline locked Cargo metadata. The first-party
crates use MIT OR Apache-2.0. Dependencies are obtained from crates.io; no cloned
third-party code is executed. No new dependency was added for image diagnostics
or reference rendering.

| Package | Version | Declared license |
| --- | --- | --- |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| bcn-decode | 0.1.0 | MIT OR Apache-2.0 |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 |
| crc32fast | 1.5.4 | MIT OR Apache-2.0 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| flate2 | 1.1.11 | MIT OR Apache-2.0 |
| generic-array | 0.14.7 | MIT |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| libc | 0.2.191 | MIT OR Apache-2.0 |
| lz4_flex | 0.14.0 | MIT |
| memchr | 2.8.3 | Unlicense OR MIT |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| simd-adler32 | 0.3.10 | MIT |
| syn | 3.0.9 | MIT OR Apache-2.0 |
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
