//! Canonical static-frame decisions, shared verbatim with spec/scene.md.
use serde::Serialize;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Used,
    Outside,
    State,
    Unknown,
}
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub class: Class,
    pub layout: &'static str,
    pub source: &'static str,
    pub reason: &'static str,
    pub pending_live_proof: &'static str,
}
pub const TABLE: &str = include_str!("../../../spec/static-records.tsv");
#[derive(Clone, Copy)]
struct Entry {
    key: u64,
    start: usize,
    end: usize,
}
const fn index() -> [Entry; 1024] {
    let mut out = [Entry {
        key: u64::MAX,
        start: 0,
        end: 0,
    }; 1024];
    let bytes = TABLE.as_bytes();
    let mut start = 0;
    let mut count = 0;
    while start < bytes.len() {
        let mut end = start;
        while end < bytes.len() && bytes[end] != b'\n' {
            end += 1;
        }
        if bytes[start] != b'#' && end > start {
            assert!(end - start >= 10 && bytes[start + 4] == b'.' && bytes[start + 9] == b'\t');
            assert!(count < 1024);
            let mut key = 0u64;
            let mut j = 0;
            while j < 9 {
                if j != 4 {
                    key = (key << 8) | bytes[start + j] as u64;
                }
                j += 1;
            }
            if count > 0 {
                assert!(out[count - 1].key < key);
            }
            out[count] = Entry { key, start, end };
            count += 1;
        }
        start = end + 1;
    }
    out
}
static INDEX: [Entry; 1024] = index();
pub fn rule(record: &[u8; 4], tag: &[u8; 4]) -> Option<Rule> {
    let key = (u32::from_be_bytes(*record) as u64) << 32 | u32::from_be_bytes(*tag) as u64;
    let i = INDEX.binary_search_by_key(&key, |e| e.key).ok()?;
    let e = INDEX[i];
    let mut fields = TABLE[e.start..e.end].split('\t');
    fields.next()?;
    let class = match fields.next()? {
        "used" => Class::Used,
        "outside" => Class::Outside,
        "state" => Class::State,
        "unknown" => Class::Unknown,
        _ => return None,
    };
    Some(Rule {
        class,
        layout: fields.next()?,
        source: fields.next()?,
        reason: fields.next()?,
        pending_live_proof: fields.next().unwrap_or("-"),
    })
}
pub fn classify_size(record: &[u8; 4], tag: &[u8; 4], size: usize) -> Class {
    if crate::ficha::schema(record, tag).is_some_and(|s| !s.accepts(size)) {
        return Class::Unknown;
    }
    classify(record, tag)
}
pub fn classify(record: &[u8; 4], tag: &[u8; 4]) -> Class {
    rule(record, tag).map_or(Class::Unknown, |r| r.class)
}
