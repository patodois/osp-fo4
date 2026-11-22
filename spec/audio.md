# Sound files and sound records

First step of the audio phase: what the game's sound data is, how much of
it `fo4-audio` reads, and which records tie it to the world. Nothing here
plays a sound; the viewer has no audio output.

`fo4_audio::Sound::read` reads a whole sound file into its container
(RIFF `WAVE`, RIFF `XWMA` or the voice container `FUZE`), its `fmt ` fields,
its chunk list and, for `XWMA`, its `dpds` table; every byte of a file
belongs to a counted part (`fo4_audio::Parts`) or the file is refused with a
named error. Integer PCM is decoded (`fo4_audio::pcm::decode`); the
WMA-family payload is not.

Every statement below comes from measuring the stored bytes of one
installation. The container layouts were recognised from prior general
knowledge before they were measured (see [provenance](../docs/provenance.md)
and the list at the end); each is a pending rule in
[hypotheses.tsv](hypotheses.tsv) and is kept only where the census, which
could have refuted it, holds. No decoder, tool, format write-up, wiki page,
source code of any implementation or the game executable was opened or
searched for. Field names are this project's labels; record and subrecord
signatures are the stored four-character codes.

Evidence tags as in [havok-container.md](havok-container.md):
**[measured N]** holds for N items and fails for none unless a second count
says so; **[hypothesis]**; **[open]**.

## Census scope

`audio_census MANIFEST OUT_DIR [CELL[:RADIUS]...]` (an example of
`fo4-audio`) reads every entry of every mounted archive one at a time
through `fo4-vfs` (no archive is held in memory), sniffs the first twelve
bytes of every entry outside the texture archives, reads every entry whose
first bytes name a sound container in full with `Sound::read`, lists the
loose files under the manifest's data root, then opens the plugins with
`fo4-plugin` for the record part and walks the cells given. It writes
counts only to `audio-census.txt`, committed as
[audio-census.txt](audio-census.txt), and names, editor ids and paths to a
second file that is not versioned. The cells are numbered there, never
named.

Same installation as [pex.md](pex.md) (build declared 1.10.163, 7 plugins,
62 archive mounts): 505,218 archive entries (41,348 in texture archives,
DDS by construction and not sniffed; 463,870 in general archives), 503,245
distinct paths. 44 s wall, of which 24 s for the files, peak RSS about
580 MB.

**Result: 170,463 sound entries (6,009,476,016 bytes) in three containers,
all read to the last byte; none refused.** 18 of them are overridden copies
(170,445 distinct paths). The 2,467 PCM files (1,806,548,818 bytes, 30% of
the sound bytes, 1.4% of the files) decode; the 167,996 files whose payload
is WMA-family (4,202,927,198 bytes) are framed and counted, not decoded. None of
the 86 loose files under the data root (their extensions are counted in the
census) is a sound container.

| Container | First bytes | Extension | Files | Bytes | Codec | Decoded |
| --- | --- | --- | ---: | ---: | --- | --- |
| RIFF `WAVE` | `RIFF`, form `WAVE` | `wav` (all) | 2,467 | 1,806,548,818 | tag 1, integer PCM | yes, 2,467 |
| RIFF `XWMA` | `RIFF`, form `XWMA` | `xwm` (all) | 9,931 | 596,176,762 | tag 0x161 | no [open] |
| `FUZE` | `FUZE` | `fuz` (all) | 158,065 | 3,606,750,436 | lip half raw; audio half `XWMA`, tag 0x161 | no [open] |

Every entry with one of these extensions has the matching first bytes and
no entry with another extension does. No other sound container (Ogg, MP3,
FLAC, sound banks) occurs.

Bytes by part over all sound files: voice-container headers 1,896,780; lip
halves 706,176,224; RIFF framing (file headers, chunk headers, pad bytes)
6,228,554; `fmt ` 3,063,400; `dpds` 9,687,756; `data` 5,282,133,386; other
chunks 289,916.

## Containers

### RIFF framing (`audio_riff_framing_v1`)

`RIFF`, u32 little-endian size, four-byte form, then chunks: four-byte id,
u32 size, that many bytes, one pad byte after an odd size. The reader
requires the size word to equal the file length minus 8, every chunk to end
inside it and the chunks to reach its last byte. **[measured 170,463]**
(whole files and the audio halves of voice containers). 79,494 chunks have
an odd size, each followed by a pad byte, all zero.

### The format chunk (`audio_format_chunk_v1`, `audio_format_tags_v1`)

| Offset | Field | Values measured |
| ---: | --- | --- |
| 0 | u16 format tag | 1 in 2,467 `WAVE`; 0x161 in 167,996 `XWMA` and voice audio halves; no other |
| 2 | u16 channels | 1 or 2 |
| 4 | u32 sample rate | 8,000, 16,000, 32,000, 44,100, 48,000, 96,000 |
| 8 | u32 average bytes per second | PCM: rate x block align in 2,467 of 2,467; tag 0x161: 4,000, 6,000, 8,000 or 12,000 |
| 12 | u16 block align | PCM: channels x bits / 8; tag 0x161: 1,008, 1,487, 2,230, 2,304, 2,731, 4,459 |
| 14 | u16 bits per sample | 16 (24 in 6 PCM files) |
| 16 | u16 extension count, then that many bytes | absent (16-byte chunk) in every `WAVE`; present with count 0 (18-byte chunk) in every tag-0x161 file |

The full histogram of tag, channels, rate, bits, block align and extension
per container is in the census. The reader refuses a 17-byte chunk and an
extension count that does not end the chunk; neither occurs.

### `WAVE` chunks

Exactly one `fmt ` and one `data` per file **[measured 2,467]**; `fmt `
comes before `data` in all, first in 2,019 (448 files start with a `JUNK`
chunk). Other chunks are kept as spans: `JUNK` (1,046 files, 28 bytes
each), `LIST` (1,257), `smpl` (935), `CDif` (255 files, 532 chunks, 68 bytes
each), `cue ` (173), `inst` (70, 7 bytes and a pad), `bext`, `Fake` and
`iXML` (one file each). Candidate views, applied by the census to every
chunk of their id and used by nothing else:

- `smpl` (`audio_wave_smpl_v1`): nine u32 words, the eighth a loop count,
  the ninth a byte count of trailing data, then the loops (six u32 words:
  cue id, kind, start, end, fraction, play count) and the trailing bytes.
  Frames 935 of 935 exactly; 901 chunks hold one loop and 34 none; kind,
  fraction and play count 0 in every loop; start <= end and end below the
  file's frame count in 901 of 901; end equals the last frame in 343.
  Word 3 is 60 or 72. Word 2 equals floor(1e9 / rate) in only 70 of 935,
  so it is not read as a sample period.
- `cue ` (`audio_wave_cue_v1`): a u32 count and that many points of six
  u32 words; 173 of 173 frame exactly.
- `LIST` (`audio_wave_list_v1`): a list type (`INFO` in 1,250, `adtl` in 7)
  and a chunk sequence filling the payload; 1,257 of 1,257.

### `XWMA` (`audio_xwma_packets_v1`, `audio_xwma_dpds_v1`)

Chunk order `fmt `, `dpds`, `data` in every file and voice audio half
**[measured 167,996]**. `dpds` is read as u32 words (its size is a multiple
of 4 in all). Measured relations:

| Check | Holds | Fails |
| --- | ---: | ---: |
| `data` length a multiple of block align (whole packets) | 167,996 | 0 |
| `dpds` entries == packets (`data` / block align) | 167,996 | 0 |
| `dpds` strictly increasing | 161,084 | 6,912 |
| ... where it is not: only the last entry equals the one before | 6,910 | |
| last `dpds` a multiple of 2 x channels | 167,996 | 0 |
| `data` / average bytes per second within 10% of last `dpds` / (2 x channels x rate) | 6,231 | 161,765 |

So the packet framing and one `dpds` word per packet hold everywhere. The
reading of a `dpds` word as decoded 16-bit bytes up to the end of its packet
is not contradicted by the last two rows but not confirmed either: the
stored average byte rate does not predict the duration it implies (most
files fall between 1.1 and 2 times it). Under that reading the `XWMA` files
hold 76,209 s and the voice audio halves 560,721 s. 2,421,939 packets in
all. **[open]**: the packet contents.

### Voice container (`audio_fuz_layout_v1`)

`FUZE`, u32 version, u32 lip length, the lip half, then the audio half to
the file's last byte, itself a RIFF file. **[measured 158,065]**: version 1
in all; every audio half an `XWMA` file read to its last byte with no byte
between the halves; 6,244 lip halves empty, the others between 128 and 268,642 bytes
(706,176,224 in all). The lip half is kept raw; its first four bytes are
`01 00 00 00` in 150,139 files and `54 4f 4d 33` in 1,682. **[open]**: the
lip half's layout.

**Seconds from the header** (`fo4_audio::header_seconds`): PCM whole
frames over the rate; the WMA family the last `dpds` word over 2 x
channels x rate (`audio_xwma_dpds_seconds_v1`). For the voice files the
stored `data` bytes fitted on those seconds give 4,101 bytes per second
against the declared 4,000 (mono) and 7,982 against 8,000 (stereo), with a
near-constant two-packet intercept, which supports the reading without
decoding ([voice.md](voice.md), Duration).

## Decoding

| Format | Files | Status |
| --- | ---: | --- |
| Tag 1, 16-bit, 1 or 2 channels | 2,461 | decoded (`pcm::decode`) |
| Tag 1, 24-bit, 2 channels | 6 | decoded |
| Tag 0x161 (WMA family) in `XWMA` | 9,931 | not decoded: packets framed and counted, payload raw **[open]** |
| Tag 0x161 in voice audio halves | 158,065 | same **[open]** |
| ADPCM (tags 2 and 0x11), any other tag | 0 | none occurs; no decoder written |

`audio_pcm_layout_v1`: frames of block-align bytes, one sample per channel
in channel order, each bits/8 bytes of little-endian two's complement. The
decoder takes only what the file declares and refuses a width other than 16
or 24, a block align other than channels x bits/8, an extension and a
partial frame. Checks over the decoded files: `data` holds whole frames in
2,467 of 2,467; the sum of steps between consecutive samples of a channel
is smaller under the candidate than under the big-endian reading in 2,466
of 2,466 and than under the offset-binary reading in 2,466 of 2,466 (one
file has no non-zero step). 630,069,108 frames, 13,167 s.

`sound_dump MANIFEST PATH [OUT.wav]` prints one file's container and, for
PCM, writes the decoded samples as a plain `WAVE` file (16-byte `fmt `, one
`data` chunk) at the source's width; it refuses an output inside a Git
working tree. For the one PCM file the test cells reach (below) the written
`data` chunk equals the source's byte for byte, and an independent system
reader reports the same format, frame count and duration. The synthetic
tests build 16- and 24-bit files in code and check the round trip, the
sample values at the extremes, the refusals, the framing errors, the
`XWMA` packet count and the voice-container split.

## Records

Census over the winning records of the 7 plugins (counts per subrecord,
sizes and text extensions are in the census for every type below).

| Type | Live | Role here |
| --- | ---: | --- |
| `SNDR` | 7,428 | sound descriptor: file paths (`ANAM`, `FNAM`), links below |
| `SOUN` | 339 | sound marker base (placed as an emitter) |
| `SNCT` | 50 | category |
| `SOPM` | 402 | output model |
| `MUSC` | 132 | music type |
| `MUST` | 624 | music track: file paths (`ANAM`, `BNAM`) |
| `ASPC` | 98 | acoustic space |
| `AECH`, `REVB` | 31, 59 | named by `SOPM ENAM` and `ASPC BNAM` |
| `REGN` | 153 | region: sounds (`RDSA`) and music (`RDMO`) |
| `CELL`, `WRLD`, `LCTN` | 62,393, 13, 487 | assignments on cells, worlds and locations |
| `DOOR`, `ACTI`, `WEAP` | 442, 2,195, 480 | sound fields |

Roles are labels for the pending rule `audio_record_links_v1`; what the game
does with each link is not measured.

### Links (`audio_record_links_v1`)

Every four-byte value at every byte offset of every subrecord of these types
was resolved against the load order; the census lists the offsets where at
least 2 values and 5% of the subrecord's occurrences name a sound-family
record. From that scan, the links:

| From | Subrecord | Slots | To | Subrecords | Slots naming the target |
| --- | --- | --- | --- | ---: | ---: |
| `CELL` | `XCAS` | word 0 | `ASPC` | 333 | 333 |
| `CELL` | `XCMO` | word 0 | `MUSC` | 330 | 330 |
| `CELL` | `XCCM` | word 0 | `REGN` | 86 | 86 |
| `CELL` | `XCLR` | every word | `REGN` | 22,815 | 39,554 |
| `CELL` | `XLCN` | word 0 | `LCTN` | 1,236 | 1,236 |
| `LCTN` | `NAM1` | word 0 | `MUSC` | 67 | 67 |
| `WRLD` | `ZNAM` | word 0 | `MUSC` | 6 | 6 |
| `REGN` | `RDMO` | word 0 | `MUSC` | 28 | 28 |
| `REGN` | `RDSA` | every 12 bytes from 0 | `SNDR` | 45 | 298 |
| `SOUN` | `SDSC` | word 0 | `SNDR` | 339 | 339 |
| `ASPC` | `SNAM` / `BNAM` / `RDAT` | word 0 | `SNDR` / `REVB` / `REGN` | 79 / 92 / 61 | all |
| `DOOR` | `SNAM` / `ANAM` | word 0 | `SNDR` | 325 / 319 | all |
| `ACTI` | `SNAM` / `VNAM` | word 0 | `SNDR` | 2 / 26 | all |
| `ACTI` | `RADR` (13 or 14 bytes) | word 0 | `SOPM` | 114 | 64 (50 zero) |
| `WEAP` | `YNAM` / `ZNAM` | word 0 | `SNDR` | 180 / 180 | all |
| `WEAP` | `DNAM` (132 bytes) | bytes 73, 85, 89, 93, 97, 101 | `SNDR` | 480 | 787 (the rest zero; bytes 77 and 81 zero in all) |
| `MUSC` | `TNAM` | every word | `MUST` | 132 | 461 |
| `MUST` | `SNAM` | every word | `MUST` | 41 | 855 (63 zero) |
| `SNDR` | `DNAM` / `SNAM` | word 0 | `SNDR` | 1,742 / 66 | all |
| `SNDR` | `BNAM` of 4 bytes | word 0 | `SNDR` | 24 (6,731 others are 6 bytes) | 24 |
| `SNDR` | `GNAM` / `ONAM` | word 0 | `SNCT` / `SOPM` | 6,733 / 6,731 | all |

No slot of these names another type or no live record. Size checks (a whole
number of words or 12-byte entries) hold in every subrecord. The `WEAP
DNAM` sound words sit at offsets that are not multiples of four and were
found only by the every-offset scan. Placed references carry no sound link
of their own besides script properties (`VMAD`) and `XEMI`, which names a
region in 14,746 placed references (not followed).

### Stored paths (`audio_record_path_readings_v1`)

Text subrecords with a `wav`, `xwm` or `fuz` extension occur only in `SNDR
ANAM`, `SNDR FNAM`, `MUST ANAM` and `MUST BNAM`; all store `.wav` and
begin, almost always, with a `data` folder. Readings tried, in order: the
path as stored, with a leading `data` folder removed, with `sound/` or
`music/` prefixed; each with the stored extension, then `.xwm`, then `.fuz`;
matched case-insensitively with either separator against every mounted
path.

| Field | Occurrences | Distinct | Resolve | As stored `.wav` (data folder removed) | As `.xwm` (data folder removed) | Other reading | Unresolved | PCM / WMA |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `SNDR ANAM` | 14,016 | 11,687 | 13,213 (94.3%) | 2,592 | 10,613 | 8 (`sound/` prefixed) | 803 | 2,594 / 10,619 |
| `SNDR FNAM` | 120 | 109 | 115 | 115 | 0 | 0 | 5 | 115 / 0 |
| `MUST ANAM` | 575 | 541 | 527 (91.7%) | 0 | 527 | 0 | 48 | 0 / 527 |
| `MUST BNAM` | 20 | 10 | 20 | 0 | 20 | 0 | 0 | 0 / 20 |

No path resolves under two readings, and no stored path resolves to a
voice container (those are reached through dialogue, not surveyed here).

### Placed references

Of 1,778,974 winning live placed `REFR`: 3,718 have a `SOUN` base (sound
emitters), 1,269 an `ASPC` base, 3,616 a `DOOR` base with a sound link
(1,733 without), 1,155 an `ACTI` base with a sound link (28,533 without).

## Test cells

The project's two test cells (named in the tools' arguments, numbered in
the census): cell 1 is the interior test cell, cell 2 the exterior test
cell with the exterior cells within one grid step (9 cells).

**Cell 1** places 454 references and nothing that names a sound: no
emitter, no acoustic space, no door or activator with a sound link, and the
cell assigns no acoustic space, music or region. Nothing to play.

**Cell 2** (2,531 placed references), walked from each origin along the
links above:

| Origin | Placed or assigned | Reached | File paths | Resolve | Decoded to PCM |
| --- | ---: | --- | ---: | ---: | ---: |
| Sound emitters (`SOUN` base) | 15 | 2 `SOUN`, 4 `SNDR` (1 without a path) | 18 (10 distinct) | 18 | 0 (all WMA) |
| Acoustic space (`ASPC` base) | 1 | 1 `ASPC` without `SNAM`, 1 `REVB`, 1 `REGN` | 0 | | |
| Doors | 14 (6 with `SNAM` and `ANAM`) | 4 `DOOR`, 2 `SNDR` | 4 | 4 | 0 (all WMA) |
| Activators | 21 (none with a sound link) | | | | |
| Cell `XCAS`, `XCMO` | 0 | | | | |
| Regions (`XCLR`) | 50 slots | 6 `REGN`, 42 `SNDR`, 2 `MUSC`, 86 `MUST` | 172 `SNDR` + 67 `MUST` | all | 6 (`SNDR`) |
| Location (`XLCN`) | 6 | 1 `LCTN`, 1 `MUSC`, 76 `MUST` | 65 | 65 | 0 |
| World music (`ZNAM`) | 1 | 1 `MUSC`, 141 `MUST` | 132 | 132 | 0 |

Every path reached in cell 2 resolves. The only decodable sound there is
one PCM file (2 channels, 48 kHz, 24-bit, 12.7 s, one `smpl` loop) named by
six region sound descriptors; everything else the cell places or assigns is
WMA-family and stays undecoded. This is the input for a later "first sound"
step; which descriptor the game plays, when and how loud are not measured.

## Open

- The WMA-family payload (tag 0x161): 167,996 files, 70% of the sound
  bytes, every door, emitter and music track of the test cells. Decoding it
  needs a codec this project has not written; no external decoder was used
  or reproduced.
- The meaning of a `dpds` word and the duration of a tag-0x161 file.
- The lip half of the voice container (706 MB), and how dialogue reaches
  voice files.
- 803 `SNDR ANAM`, 5 `SNDR FNAM` and 48 `MUST ANAM` paths name no mounted
  file under any reading.
- What the game does with each link and field: categories, output models,
  conditions (`CTDA`), the other subrecords of `SNDR`, `MUSC`, `MUST`,
  `ASPC`, `REGN RDSA` bytes 4 to 11, `smpl` loops, `cue ` points.

## Rules registered

`audio_riff_framing_v1`, `audio_format_chunk_v1`, `audio_format_tags_v1`,
`audio_pcm_layout_v1`, `audio_xwma_packets_v1`, `audio_xwma_dpds_v1`,
`audio_fuz_layout_v1`, `audio_wave_smpl_v1`, `audio_wave_cue_v1`,
`audio_wave_list_v1`, `audio_record_links_v1`,
`audio_record_path_readings_v1`, all pending, in
[hypotheses.tsv](hypotheses.tsv).

## Provenance

No format fact here has a cited source. Recognised from prior general
knowledge before measurement, and kept only because the census could have
refuted it and did not:

- RIFF chunking: the magic, the size word, four-byte chunk ids, u32 chunk
  sizes, the pad byte after an odd chunk, `LIST` holding a list type and
  sub-chunks.
- The `WAVE` and `XWMA` form types and the chunk ids `fmt `, `data`,
  `dpds`, `smpl`, `cue `, `LIST`, `JUNK`.
- The `fmt ` layout (six fields and the extension count) and the format
  tag numbers 1 (integer PCM), 2 and 0x11 (ADPCM variants), 3 (float),
  0x161 and 0x162 (WMA family), 0xFFFE (extensible); only 1 and 0x161
  occur.
- PCM as interleaved little-endian two's-complement frames (decided over
  the alternatives by the step checks above, not taken from memory).
- `dpds` as one u32 per packet counting decoded bytes; `XWMA` data as
  packets of block-align bytes.
- The voice container as a `FUZE` magic, version, lip length, lip data and
  audio data.
- The `smpl` and `cue ` layouts.
- That sound descriptors store paths with a `.wav` extension for files
  stored compressed under another extension, and that paths may carry a
  leading `data` folder.
- The record signatures and roles named above (sound descriptor, marker,
  category, output model, music type and track, acoustic space, region
  sound and music); the subrecord offsets and targets were measured, not
  recalled, and the `WEAP DNAM` offsets were found by the scan.

The independent system reader used once on the dumped file is a check of
this project's output, not a reference for the format.
