//! Synthetic files built in code; no game data.
use crate::format::{WaveFormat, TAG_PCM, TAG_WMA2};
use crate::pcm::{self, Pcm, Status};
use crate::wave::{self, Cue, Smpl};
use crate::{fuz, packets, Container, Sound};
use fo4_core::Budget;

fn budget() -> Budget {
    Budget::new(1 << 26)
}

fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    v.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        v.push(0);
    }
    v
}

fn riff(form: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut v = b"RIFF".to_vec();
    v.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    v.extend_from_slice(form);
    v.extend_from_slice(&body);
    v
}

fn fmt(tag: u16, ch: u16, rate: u32, align: u16, bits: u16, ext: Option<&[u8]>) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&tag.to_le_bytes());
    v.extend_from_slice(&ch.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
    v.extend_from_slice(&align.to_le_bytes());
    v.extend_from_slice(&bits.to_le_bytes());
    if let Some(e) = ext {
        v.extend_from_slice(&(e.len() as u16).to_le_bytes());
        v.extend_from_slice(e);
    }
    v
}

fn tone(channels: u16, bits: u16, frames: usize) -> Pcm {
    let amp = if bits == 16 { 30_000 } else { 8_000_000 };
    let samples = (0..frames * usize::from(channels))
        .map(|i| {
            let x = (i as i64 * 7919 % 2001) - 1000;
            (x * amp / 1000) as i32
        })
        .collect();
    Pcm {
        channels,
        sample_rate: 48_000,
        bits,
        samples,
    }
}

#[test]
fn pcm_round_trip_16_and_24_bits() {
    for (ch, bits) in [(1u16, 16u16), (2, 16), (2, 24), (1, 24)] {
        let p = tone(ch, bits, 101);
        let bytes = pcm::encode_wav(&p, &budget()).unwrap();
        let s = Sound::read(&bytes, "synthetic", &budget()).unwrap();
        assert_eq!(s.container, Container::Wave);
        assert_eq!(s.parts.total(), bytes.len() as u64);
        assert_eq!(s.status(), Status::Pcm);
        let back = s.decode(&bytes, "synthetic", &budget()).unwrap();
        assert_eq!(back, p, "{ch} channels, {bits} bits");
        assert_eq!(back.frames(), 101);
        // Writing the decoded samples again gives the same file.
        assert_eq!(pcm::encode_wav(&back, &budget()).unwrap(), bytes);
    }
}

#[test]
fn pcm_sample_values_are_little_endian_twos_complement() {
    let data = [0x00, 0x80, 0xff, 0x7f, 0xfe, 0xff];
    let f = WaveFormat::parse(&fmt(TAG_PCM, 1, 8000, 2, 16, None), "f", 0).unwrap();
    let p = pcm::decode(&f, &data, "f", 0, &budget()).unwrap();
    assert_eq!(p.samples, vec![-32768, 32767, -2]);
    let f = WaveFormat::parse(&fmt(TAG_PCM, 1, 8000, 3, 24, None), "f", 0).unwrap();
    let p = pcm::decode(&f, &[0x00, 0x00, 0x80, 0xff, 0xff, 0x7f], "f", 0, &budget()).unwrap();
    assert_eq!(p.samples, vec![-(1 << 23), (1 << 23) - 1]);
}

#[test]
fn pcm_refuses_what_was_not_measured() {
    let ok = |tag, ch, align, bits, ext: Option<&[u8]>| {
        let f = WaveFormat::parse(&fmt(tag, ch, 8000, align, bits, ext), "f", 0).unwrap();
        pcm::decode(&f, &[0u8; 12], "f", 0, &budget()).map_err(|e| e.code)
    };
    assert_eq!(ok(TAG_PCM, 1, 1, 8, None).unwrap_err(), "PcmBits");
    assert_eq!(ok(TAG_PCM, 2, 2, 16, None).unwrap_err(), "PcmBlockAlign");
    assert_eq!(
        ok(TAG_PCM, 1, 2, 16, Some(&[])).unwrap_err(),
        "PcmExtension"
    );
    assert_eq!(ok(TAG_WMA2, 1, 2, 16, None).unwrap_err(), "PcmTag");
    let f = WaveFormat::parse(&fmt(TAG_PCM, 2, 8000, 4, 16, None), "f", 0).unwrap();
    let e = pcm::decode(&f, &[0u8; 6], "f", 0, &budget()).unwrap_err();
    assert_eq!(e.code, "PcmPartialFrame");
}

#[test]
fn riff_framing_is_strict() {
    let good = riff(
        b"WAVE",
        &[
            chunk(b"fmt ", &fmt(TAG_PCM, 1, 8000, 2, 16, None)),
            chunk(b"data", &[1, 2, 3]),
            chunk(b"JUNK", &[0; 5]),
        ],
    );
    let s = Sound::read(&good, "g", &budget()).unwrap();
    assert_eq!(s.parts.data, 3);
    assert_eq!(s.parts.other, 5);
    assert_eq!(s.parts.framing, 12 + 3 * 8 + 2);
    assert_eq!(s.wave.riff.chunks[1].pad, Some(0));
    let code = |b: &[u8]| Sound::read(b, "x", &budget()).unwrap_err().code;
    // A trailing byte the size word does not frame.
    let mut long = good.clone();
    long.push(0);
    assert_eq!(code(&long), "RiffLength");
    // An odd last chunk without its pad byte.
    let mut short = good.clone();
    short.pop();
    let n = (short.len() - 8) as u32;
    short[4..8].copy_from_slice(&n.to_le_bytes());
    assert_eq!(code(&short), "ChunkPadMissing");
    // A chunk claiming more bytes than the file holds.
    let mut over = good.clone();
    over[16..20].copy_from_slice(&1000u32.to_le_bytes());
    assert_eq!(code(&over), "ChunkPastEnd");
    // No data chunk.
    let no_data = riff(
        b"WAVE",
        &[chunk(b"fmt ", &fmt(TAG_PCM, 1, 8000, 2, 16, None))],
    );
    assert_eq!(code(&no_data), "ChunkMissing");
    // A fmt extension whose count disagrees with the chunk.
    let bad_ext = riff(
        b"WAVE",
        &[
            chunk(
                b"fmt ",
                &[fmt(TAG_PCM, 1, 8000, 2, 16, None), vec![4, 0]].concat(),
            ),
            chunk(b"data", &[]),
        ],
    );
    assert_eq!(code(&bad_ext), "FormatExtension");
    assert_eq!(code(b"OggS0000"), "SoundMagic");
}

fn xwma(packets_n: usize, align: u16) -> Vec<u8> {
    let dpds: Vec<u8> = (1..=packets_n as u32)
        .flat_map(|i| (i * 4096).to_le_bytes())
        .collect();
    riff(
        b"XWMA",
        &[
            chunk(b"fmt ", &fmt(TAG_WMA2, 1, 44_100, align, 16, Some(&[]))),
            chunk(b"dpds", &dpds),
            chunk(b"data", &vec![0x5a; packets_n * usize::from(align)]),
        ],
    )
}

#[test]
fn xwma_counts_packets_and_keeps_the_payload_raw() {
    let bytes = xwma(3, 31);
    let s = Sound::read(&bytes, "x", &budget()).unwrap();
    assert_eq!(s.container, Container::Xwma);
    assert_eq!(s.status(), Status::WmaOpen);
    assert_eq!(s.wave.dpds, vec![4096, 8192, 12288]);
    assert_eq!(packets(&s.wave), Some(3));
    assert_eq!(s.parts.seek, 12);
    assert_eq!(s.parts.data, 93);
    assert_eq!(s.parts.total(), bytes.len() as u64);
    assert_eq!(s.decode(&bytes, "x", &budget()).unwrap_err().code, "PcmTag");
    // A dpds chunk that is not whole words is refused.
    let mut bad = riff(
        b"XWMA",
        &[
            chunk(b"fmt ", &fmt(TAG_WMA2, 1, 44_100, 31, 16, Some(&[]))),
            chunk(b"dpds", &[1, 2, 3]),
            chunk(b"data", &[]),
        ],
    );
    assert_eq!(
        Sound::read(&bad, "x", &budget()).unwrap_err().code,
        "DpdsSize"
    );
    bad[8..12].copy_from_slice(b"WAVE");
    assert_eq!(
        Sound::read(&bad, "x", &budget()).unwrap_err().code,
        "DpdsInWave"
    );
}

#[test]
fn fuz_splits_lip_and_audio() {
    let audio = xwma(2, 17);
    for lip in [vec![], vec![1, 0, 0, 0, 9, 9, 9]] {
        let mut bytes = b"FUZE".to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(lip.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&lip);
        bytes.extend_from_slice(&audio);
        let s = Sound::read(&bytes, "v", &budget()).unwrap();
        assert_eq!(s.container, Container::Fuz);
        let f = s.fuz.unwrap();
        assert_eq!(f.version, 1);
        assert_eq!(f.lip.of(&bytes).unwrap(), &lip[..]);
        assert_eq!(s.audio_offset, 12 + lip.len() as u64);
        assert_eq!(s.parts.lip, lip.len() as u64);
        assert_eq!(s.parts.fuz_header, fuz::HEADER_BYTES);
        assert_eq!(s.parts.total(), bytes.len() as u64);
        assert_eq!(s.data(&bytes).unwrap(), &[0x5a; 34][..]);
        // An error inside the audio half names its offset in the file.
        let mut broken = bytes.clone();
        let at = s.audio_offset as usize + 4;
        broken[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
        let e = Sound::read(&broken, "v", &budget()).unwrap_err();
        assert_eq!(e.code, "RiffLength");
        assert_eq!(e.source.offset, at as u64);
    }
    let mut past = b"FUZE".to_vec();
    past.extend_from_slice(&1u32.to_le_bytes());
    past.extend_from_slice(&100u32.to_le_bytes());
    assert_eq!(
        Sound::read(&past, "v", &budget()).unwrap_err().code,
        "FuzLipPastEnd"
    );
}

#[test]
fn chunk_views_frame_exactly_or_refuse() {
    let mut smpl = Vec::new();
    for w in [0u32, 0, 20833, 60, 0, 0, 0, 1, 2] {
        smpl.extend_from_slice(&w.to_le_bytes());
    }
    for w in [7u32, 0, 100, 900, 0, 0] {
        smpl.extend_from_slice(&w.to_le_bytes());
    }
    smpl.extend_from_slice(&[0xaa, 0xbb]);
    let s = Smpl::parse(&smpl).unwrap();
    assert_eq!(s.loops.len(), 1);
    assert_eq!((s.loops[0].start, s.loops[0].end), (100, 900));
    assert_eq!(s.trailing, vec![0xaa, 0xbb]);
    assert!(Smpl::parse(&smpl[..smpl.len() - 1]).is_none());

    let mut cue = 2u32.to_le_bytes().to_vec();
    cue.extend((0..12u32).flat_map(|w| w.to_le_bytes()));
    assert_eq!(Cue::parse(&cue).unwrap().points[1], [6, 7, 8, 9, 10, 11]);
    assert!(Cue::parse(&cue[..cue.len() - 4]).is_none());

    let list = [
        b"INFO".to_vec(),
        chunk(b"ICMT", b"abc"),
        chunk(b"ISFT", b"xy"),
    ]
    .concat();
    let (kind, sub) = wave::list(&list, &budget()).unwrap();
    assert_eq!(&kind, b"INFO");
    assert_eq!(sub.len(), 2);
    assert_eq!(sub[0].pad, Some(0));
    assert!(wave::list(&list[..list.len() - 1], &budget()).is_none());
}
