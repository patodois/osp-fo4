//! Counting helpers for the census text.
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub type Counts = BTreeMap<String, u64>;
pub fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
pub fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}
/// Holds and fails of a named check.
pub type Checks = BTreeMap<String, [u64; 2]>;
pub fn check(m: &mut Checks, k: impl Into<String>, ok: bool) {
    m.entry(k.into()).or_default()[usize::from(!ok)] += 1;
}

/// Count, minimum, maximum and sum of a value.
#[derive(Default, Clone, Copy)]
pub struct Range {
    pub n: u64,
    pub min: u64,
    pub max: u64,
    pub sum: u64,
}
impl Range {
    pub fn put(&mut self, v: u64) {
        if self.n == 0 || v < self.min {
            self.min = v;
        }
        self.max = self.max.max(v);
        self.n += 1;
        self.sum += v;
    }
}
impl std::fmt::Display for Range {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n {} min {} max {} sum {}",
            self.n, self.min, self.max, self.sum
        )
    }
}

pub fn section(out: &mut String, title: &str) {
    let _ = writeln!(out, "\n# {title}");
}
pub fn counts(out: &mut String, title: &str, m: &Counts) {
    section(out, title);
    for (k, v) in m {
        let _ = writeln!(out, "{v}\t{k}");
    }
}
/// The `top` largest counts of a map, then how many keys are left out.
pub fn top(out: &mut String, title: &str, m: &Counts, top: usize) {
    section(
        out,
        &format!("{title} ({} distinct, {top} most frequent)", m.len()),
    );
    let mut v: Vec<(&String, &u64)> = m.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (k, n) in v.iter().take(top) {
        let _ = writeln!(out, "{n}\t{k}");
    }
    let rest: u64 = v.iter().skip(top).map(|(_, n)| **n).sum();
    if rest > 0 {
        let _ = writeln!(out, "{rest}\t(other values)");
    }
}
pub fn checks(out: &mut String, title: &str, m: &Checks) {
    section(out, &format!("{title} (holds, fails)"));
    for (k, [h, f]) in m {
        let _ = writeln!(out, "{h}\t{f}\t{k}");
    }
}

/// Histogram of a signed distance in fixed buckets.
pub const EDGES: [f64; 14] = [
    -4096.0, -1024.0, -256.0, -64.0, -16.0, -4.0, -1.0, 1.0, 4.0, 16.0, 64.0, 256.0, 1024.0, 4096.0,
];
pub fn bucket(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    let i = EDGES.partition_point(|e| *e <= v);
    let lo = if i == 0 {
        "-inf".to_string()
    } else {
        format!("{}", EDGES[i - 1])
    };
    let hi = if i == EDGES.len() {
        "+inf".to_string()
    } else {
        format!("{}", EDGES[i])
    };
    format!("[{lo:>6}, {hi:>6})")
}
/// Buckets of `values` in order, then quantiles.
pub fn distribution(out: &mut String, title: &str, values: &mut [f64]) {
    section(out, &format!("{title} ({} values)", values.len()));
    let mut m: BTreeMap<usize, (String, u64)> = BTreeMap::new();
    for v in values.iter() {
        let i = if v.is_nan() {
            usize::MAX
        } else {
            EDGES.partition_point(|e| *e <= *v)
        };
        m.entry(i).or_insert_with(|| (bucket(*v), 0)).1 += 1;
    }
    for (name, n) in m.values() {
        let _ = writeln!(out, "{n}\t{name}");
    }
    values.sort_by(|a, b| a.total_cmp(b));
    if values.is_empty() {
        return;
    }
    let q = |p: f64| values[((values.len() - 1) as f64 * p).round() as usize];
    let _ = writeln!(
        out,
        "quantiles: 1% {:.2} | 5% {:.2} | 25% {:.2} | 50% {:.2} | 75% {:.2} | 95% {:.2} | 99% {:.2}",
        q(0.01),
        q(0.05),
        q(0.25),
        q(0.5),
        q(0.75),
        q(0.95),
        q(0.99)
    );
}
