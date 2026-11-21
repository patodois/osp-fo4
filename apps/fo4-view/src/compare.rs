//! CPU frame against GPU frame of the same pose and size. Prints numbers; no
//! pass threshold is defined.
//!
//! Coverage ("drew something") comes from the per-pixel instance tags when
//! both tag files are given (`u32::MAX` is background, as in
//! `Frame::instance_bytes`). Without tags it falls back to "the RGB differs
//! from the clear colour", which miscounts a surface that happens to have the
//! clear colour; the report says which one was used.

/// RGB bytes of the reference clear colour, as the CPU PNG encoder rounds it.
pub fn clear_rgb(clear: [f32; 4]) -> [u8; 3] {
    std::array::from_fn(|i| (clear[i].clamp(0., 1.) * 255.).round() as u8)
}

pub struct Inputs<'a> {
    pub size: [u32; 2],
    /// RGBA8, row-major, top row first.
    pub cpu: &'a [u8],
    pub gpu: &'a [u8],
    /// One u32 per pixel, `u32::MAX` for background.
    pub tags: Option<(&'a [u32], &'a [u32])>,
    pub clear: [u8; 3],
}

#[derive(Debug, PartialEq)]
pub struct Report {
    pub pixels: u64,
    pub coverage_from_tags: bool,
    pub both: u64,
    pub neither: u64,
    pub cpu_only: u64,
    pub gpu_only: u64,
    /// Over pixels both drew, per channel R, G, B, in 8-bit steps.
    pub mean_abs: [f64; 3],
    pub p95_abs: [u8; 3],
    pub max_abs: [u8; 3],
    /// Over pixels both drew: equal tags (only with tags).
    pub same_tag: Option<u64>,
    /// Difference image, RGB8: black where neither drew, red where only the
    /// CPU drew, blue where only the GPU drew, grey = min(255, 4 x largest
    /// channel difference) where both drew.
    pub image: Vec<[u8; 3]>,
}
impl Report {
    pub fn agreement(&self) -> f64 {
        (self.both + self.neither) as f64 / self.pixels as f64
    }
}

/// 95th percentile by nearest rank (the smallest value with at least 95% of
/// the samples at or below it), from a 256-bin histogram.
pub fn p95(histogram: &[u64; 256]) -> u8 {
    let n: u64 = histogram.iter().sum();
    if n == 0 {
        return 0;
    }
    let rank = (n * 95).div_ceil(100);
    let mut seen = 0;
    for (value, count) in histogram.iter().enumerate() {
        seen += count;
        if seen >= rank {
            return value as u8;
        }
    }
    255
}

pub fn compare(input: &Inputs) -> Result<Report, String> {
    let n = (input.size[0] as usize)
        .checked_mul(input.size[1] as usize)
        .ok_or("ComparisonSizeOverflow")?;
    if n == 0 || input.cpu.len() != n * 4 || input.gpu.len() != n * 4 {
        return Err(format!(
            "ComparisonSizeMismatch: {}x{} needs {} RGBA bytes, got CPU {} GPU {}",
            input.size[0],
            input.size[1],
            n * 4,
            input.cpu.len(),
            input.gpu.len()
        ));
    }
    if let Some((a, b)) = input.tags {
        if a.len() != n || b.len() != n {
            return Err(format!(
                "ComparisonTagMismatch: {n} pixels, CPU tags {}, GPU tags {}",
                a.len(),
                b.len()
            ));
        }
    }
    let mut report = Report {
        pixels: n as u64,
        coverage_from_tags: input.tags.is_some(),
        both: 0,
        neither: 0,
        cpu_only: 0,
        gpu_only: 0,
        mean_abs: [0.; 3],
        p95_abs: [0; 3],
        max_abs: [0; 3],
        same_tag: input.tags.map(|_| 0),
        image: Vec::with_capacity(n),
    };
    let mut histograms = [[0u64; 256]; 3];
    let mut sums = [0u64; 3];
    for i in 0..n {
        let c = &input.cpu[i * 4..i * 4 + 3];
        let g = &input.gpu[i * 4..i * 4 + 3];
        let (cpu_drew, gpu_drew) = match input.tags {
            Some((a, b)) => (a[i] != u32::MAX, b[i] != u32::MAX),
            None => (c != input.clear, g != input.clear),
        };
        let pixel = match (cpu_drew, gpu_drew) {
            (false, false) => {
                report.neither += 1;
                [0, 0, 0]
            }
            (true, false) => {
                report.cpu_only += 1;
                [255, 0, 0]
            }
            (false, true) => {
                report.gpu_only += 1;
                [0, 0, 255]
            }
            (true, true) => {
                report.both += 1;
                if let (Some(same), Some((a, b))) = (report.same_tag.as_mut(), input.tags) {
                    *same += u64::from(a[i] == b[i]);
                }
                let mut largest = 0;
                for k in 0..3 {
                    let d = c[k].abs_diff(g[k]);
                    histograms[k][d as usize] += 1;
                    sums[k] += u64::from(d);
                    report.max_abs[k] = report.max_abs[k].max(d);
                    largest = largest.max(d);
                }
                let grey = largest.saturating_mul(4);
                [grey; 3]
            }
        };
        report.image.push(pixel);
    }
    for k in 0..3 {
        report.mean_abs[k] = if report.both == 0 {
            0.
        } else {
            sums[k] as f64 / report.both as f64
        };
        report.p95_abs[k] = p95(&histograms[k]);
    }
    Ok(report)
}

/// Little-endian u32 per pixel, as `Frame::instance_bytes` writes them.
pub fn tags_from_bytes(bytes: &[u8]) -> Result<Vec<u32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err(format!(
            "TagFileLength: {} is not a multiple of 4",
            bytes.len()
        ));
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_is_nearest_rank() {
        let mut h = [0u64; 256];
        h[0] = 94;
        h[7] = 6;
        assert_eq!(p95(&h), 7);
        h[0] = 95;
        h[7] = 5;
        assert_eq!(p95(&h), 0);
        assert_eq!(p95(&[0; 256]), 0);
    }

    #[test]
    fn counts_coverage_and_colour_differences() {
        // Four pixels: both drew (diff 2,0,4), CPU only, GPU only, neither.
        let clear = [6, 6, 6];
        let cpu = [10, 20, 30, 255, 50, 50, 50, 255, 6, 6, 6, 255, 6, 6, 6, 255];
        let gpu = [12, 20, 26, 255, 6, 6, 6, 255, 90, 90, 90, 255, 6, 6, 6, 255];
        let r = compare(&Inputs {
            size: [2, 2],
            cpu: &cpu,
            gpu: &gpu,
            tags: None,
            clear,
        })
        .unwrap();
        assert_eq!((r.both, r.cpu_only, r.gpu_only, r.neither), (1, 1, 1, 1));
        assert_eq!(r.agreement(), 0.5);
        assert_eq!(r.mean_abs, [2., 0., 4.]);
        assert_eq!(r.max_abs, [2, 0, 4]);
        assert_eq!(r.image, vec![[16; 3], [255, 0, 0], [0, 0, 255], [0; 3]]);
        assert_eq!(r.same_tag, None);
        // Tags override the colour test: pixel 4 drew on both, same tag.
        let cpu_tags = [1, 2, u32::MAX, 5];
        let gpu_tags = [3, u32::MAX, 4, 5];
        let r = compare(&Inputs {
            size: [2, 2],
            cpu: &cpu,
            gpu: &gpu,
            tags: Some((&cpu_tags, &gpu_tags)),
            clear,
        })
        .unwrap();
        assert!(r.coverage_from_tags);
        assert_eq!((r.both, r.cpu_only, r.gpu_only, r.neither), (2, 1, 1, 0));
        assert_eq!(r.same_tag, Some(1));
    }

    #[test]
    fn mismatched_sizes_and_tag_files_are_errors() {
        let px = [0u8; 8];
        let e = compare(&Inputs {
            size: [2, 2],
            cpu: &px,
            gpu: &px,
            tags: None,
            clear: [0; 3],
        });
        assert!(e.is_err());
        assert_eq!(
            tags_from_bytes(&[1, 0, 0, 0, 255, 255, 255, 255]).unwrap(),
            [1, u32::MAX]
        );
        assert!(tags_from_bytes(&[0; 5]).is_err());
        assert_eq!(clear_rgb([0.025, 0.025, 0.025, 1.]), [6, 6, 6]);
    }
}
