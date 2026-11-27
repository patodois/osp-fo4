//! Material swap entries whose source path holds `*` (`spec/materials.md`,
//! Swap patterns). The swap reader keeps such paths as stored; this module
//! reads them as patterns only under the pending rule
//! [`crate::hypotheses::SWAP_PATTERN`]: over the resolved path key (lower
//! case, `/` separators, the `materials/` root) each `*` stands for any
//! run of bytes, possibly empty, and a target holding as many `*` as its
//! source takes the runs in order; a target without `*` is taken as it is.
//! Nothing here says what the game does: without the rule a pattern
//! compares as bytes, as every other path does, and matches no material.
use fo4_core::{Budget, Error, Result};

/// The most `*` a pattern may hold, the most ways one path may be split
/// by a pattern, and the most runs tried for one match, before the match
/// is refused (bounded work).
pub const MAX_STARS: usize = 8;
pub const MAX_SPLITS: usize = 64;
pub const MAX_TRIES: usize = 1 << 16;

/// Whether a resolved swap path is a pattern.
pub fn is_pattern(key: &[u8]) -> bool {
    key.contains(&b'*')
}

fn stars(key: &[u8]) -> usize {
    key.iter().filter(|c| **c == b'*').count()
}

fn refused(found: impl std::fmt::Display) -> Error {
    Error::new(
        "UnsupportedSwapPattern",
        "material swap",
        0,
        "a source pattern and a target with no `*` or as many",
        found,
    )
}

/// Every way `pattern` matches `path`: the runs its `*` stand for, in
/// order. Empty when it does not match.
pub fn captures(pattern: &[u8], path: &[u8], b: &Budget) -> Result<Vec<Vec<Vec<u8>>>> {
    let n = stars(pattern);
    if n > MAX_STARS {
        return Err(refused(format!("{n} stars")));
    }
    let mut out = Vec::new();
    let mut runs = Vec::new();
    b.reserve(&mut runs, n, "material swap", 0)?;
    walk(pattern, path, &mut runs, &mut out, &mut 0, b)?;
    Ok(out)
}

/// Whether `pattern`, from its start up to its first `*` (or whole, with
/// none), can begin `path`: the test [`walk`] makes first.
fn can_begin(pattern: &[u8], path: &[u8]) -> bool {
    match pattern.iter().position(|c| *c == b'*') {
        Some(k) => path.starts_with(&pattern[..k]),
        None => pattern == path,
    }
}

fn walk(
    pattern: &[u8],
    path: &[u8],
    runs: &mut Vec<Vec<u8>>,
    out: &mut Vec<Vec<Vec<u8>>>,
    tries: &mut usize,
    b: &Budget,
) -> Result<()> {
    let Some(k) = pattern.iter().position(|c| *c == b'*') else {
        if pattern == path {
            if out.len() == MAX_SPLITS {
                return Err(refused(format!("more than {MAX_SPLITS} splits")));
            }
            b.reserve(out, 1, "material swap", 0)?;
            let mut copy = Vec::new();
            b.reserve(&mut copy, runs.len(), "material swap", 0)?;
            for r in runs.iter() {
                let mut v = Vec::new();
                b.reserve(&mut v, r.len(), "material swap", 0)?;
                v.extend_from_slice(r);
                copy.push(v);
            }
            out.push(copy);
        }
        return Ok(());
    };
    if path.len() < k || path[..k] != pattern[..k] {
        return Ok(());
    }
    let rest = &path[k..];
    let next = &pattern[k + 1..];
    for n in 0..=rest.len() {
        // A run after which the rest of the pattern cannot begin gives no
        // split: it is not tried.
        if !can_begin(next, &rest[n..]) {
            continue;
        }
        *tries += 1;
        if *tries > MAX_TRIES {
            return Err(refused(format!("more than {MAX_TRIES} runs tried")));
        }
        // The run is held only while it is tried: its charge goes back
        // when the scope drops.
        let scope = b.child();
        let mut run = Vec::new();
        scope.reserve(&mut run, n, "material swap", 0)?;
        run.extend_from_slice(&rest[..n]);
        runs.push(run);
        walk(next, &rest[n..], runs, out, tries, b)?;
        runs.pop();
    }
    Ok(())
}

/// The target `target` gives for the runs `runs` of its source pattern:
/// as it is without `*`, its `*` filled in order when it holds as many as
/// the source; anything else is refused.
pub fn fill(target: &[u8], runs: &[Vec<u8>], b: &Budget) -> Result<Vec<u8>> {
    let n = stars(target);
    if n != 0 && n != runs.len() {
        return Err(refused(format!(
            "source with {} stars, target with {n}",
            runs.len()
        )));
    }
    let mut out = Vec::new();
    let mut runs_left = runs.iter();
    let size = target.len() + runs.iter().map(Vec::len).sum::<usize>();
    b.reserve(&mut out, size, "material swap", 0)?;
    for c in target {
        if *c == b'*' {
            if let Some(r) = runs_left.next() {
                out.extend_from_slice(r);
            }
        } else {
            out.push(*c);
        }
    }
    Ok(out)
}

/// What a pattern pair gives `path`: `None` when the source does not
/// match; the target otherwise. Refused (`AmbiguousMaterialSwap`) when the
/// path splits in more than one way and the splits give different
/// targets.
pub fn apply(source: &[u8], target: &[u8], path: &[u8], b: &Budget) -> Result<Option<Vec<u8>>> {
    let splits = captures(source, path, b)?;
    let mut formed: Option<Vec<u8>> = None;
    for runs in &splits {
        let t = fill(target, runs, b)?;
        match &formed {
            Some(f) if *f != t => {
                return Err(Error::new(
                    "AmbiguousMaterialSwap",
                    "material swap",
                    0,
                    "one target for every split of the path",
                    "pattern splits differ",
                ))
            }
            Some(_) => {}
            None => formed = Some(t),
        }
    }
    Ok(formed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stars_match_any_run_and_fill_the_target_in_order() {
        let b = Budget::new(1 << 20);
        let go = |s: &[u8], t: &[u8], p: &[u8]| apply(s, t, p, &b).unwrap();
        // One star, target without one: every match goes to the target.
        assert_eq!(
            go(b"m/rock*.bgsm", b"m/cliff.bgsm", b"m/rock01.bgsm"),
            Some(b"m/cliff.bgsm".to_vec())
        );
        // The run may be empty.
        assert_eq!(
            go(b"m/rock*.bgsm", b"m/cliff.bgsm", b"m/rock.bgsm"),
            Some(b"m/cliff.bgsm".to_vec())
        );
        assert_eq!(go(b"m/rock*.bgsm", b"m/cliff.bgsm", b"m/stone.bgsm"), None);
        // One star each side: the run carries over.
        assert_eq!(
            go(b"m/rub*.bgsm", b"m/sand*.bgsm", b"m/rubdecal.bgsm"),
            Some(b"m/sanddecal.bgsm".to_vec())
        );
        // Two stars each side, one split only.
        assert_eq!(
            go(
                b"m/a*decal*.bgsm",
                b"m/b*decal*.bgsm",
                b"m/a01decal_8bit.bgsm"
            ),
            Some(b"m/b01decal_8bit.bgsm".to_vec())
        );
        // A star may stand for separators too.
        assert_eq!(
            go(b"m/*", b"m/test/t.bgsm", b"m/x/y.bgsm"),
            Some(b"m/test/t.bgsm".to_vec())
        );
        // Two splits giving different targets are refused.
        assert_eq!(
            apply(b"m/*x*.bgsm", b"m/*y*.bgsm", b"m/axbxc.bgsm", &b)
                .unwrap_err()
                .code,
            "AmbiguousMaterialSwap"
        );
        // Two splits giving one target are not.
        assert_eq!(
            go(b"m/*x*.bgsm", b"m/t.bgsm", b"m/axbxc.bgsm"),
            Some(b"m/t.bgsm".to_vec())
        );
        // A target with another count of stars is refused.
        assert_eq!(
            apply(b"m/a*.bgsm", b"m/*b*.bgsm", b"m/a1.bgsm", &b)
                .unwrap_err()
                .code,
            "UnsupportedSwapPattern"
        );
        assert!(is_pattern(b"m/a*.bgsm") && !is_pattern(b"m/a.bgsm"));
    }

    #[test]
    fn runs_tried_are_bounded_and_their_charges_go_back() {
        // Eight stars over a long run of one byte: about 10^12 ways to try,
        // refused after MAX_TRIES runs instead.
        let b = Budget::new(1 << 20);
        let path = [b'a'; 200];
        let e = captures(b"********b", &path, &b).unwrap_err();
        assert_eq!(e.code, "UnsupportedSwapPattern");
        assert!(e.found.contains("runs tried"), "{}", e.found);
        // A pattern that does not match keeps no charge for the runs it
        // tried (they used to add up past the budget).
        let b = Budget::new(1 << 20);
        let path = [b'a'; 300];
        assert!(captures(b"*a*b", &path, &b).unwrap().is_empty());
        assert!(b.used() < 4096, "{}", b.used());
    }
}
