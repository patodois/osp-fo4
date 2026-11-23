//! Initial presence of a placed record that names an enable parent (XESP),
//! under pending rules (`spec/enable-parent.md`, `spec/hypotheses.tsv`).
//!
//! The chain is read by [`fo4_plugin::enable::chain`]. Every rule a chain
//! needs is raised as a `RuntimeEnableParent` blocker whose `found` text
//! names the part ([`FOUND_FOLLOWS`] and the others), so that readiness,
//! collision and the coverage census see each part apart; a chain the
//! rules do not decide raises a text no rule covers. Nothing here changes
//! what is drawn below the hypotheses level: every rule is pending.
use crate::hypotheses;
use fo4_core::{Budget, Error, Result, SourceSpan};
use fo4_plugin::{
    enable::{Chain, ChainEnd},
    FormKey,
};
use serde::Serialize;

/// The child follows its parent (`xesp_child_follows_parent_v1`).
pub const FOUND_FOLLOWS: &str = "XESP";
/// The parent's own start state (`xesp_initial_state_chain_v1`).
pub const FOUND_INITIAL: &str = "XESP.initial";
/// Flag bit 0 somewhere along the chain (`xesp_flag_bit0_opposite_v1`).
pub const FOUND_OPPOSITE: &str = "XESP.opposite";
/// Flag bit 1 somewhere along the chain (`xesp_flag_bit1_not_presence_v1`).
pub const FOUND_BIT1: &str = "XESP.bit1";
/// Not decided: other flag bits.
pub const FOUND_OTHER_BITS: &str = "XESP.flags";
/// Not decided: a record of the chain other than its root (the child
/// included) carries its own header flag 0x800 beside its XESP.
pub const FOUND_OWN_FLAG: &str = "XESP.own_flag";
/// Not decided: the chain does not end at a record without XESP.
pub const FOUND_CYCLE: &str = "XESP.cycle";
pub const FOUND_UNRESOLVED: &str = "XESP.unresolved";
pub const FOUND_TOO_DEEP: &str = "XESP.too_deep";

/// What the rules say about one placed record with an XESP.
#[derive(Debug, Clone, Serialize)]
pub struct Presence {
    pub chain: Chain,
    /// Whether it starts enabled; `None` when the rules do not decide.
    pub enabled: Option<bool>,
}
impl Presence {
    /// The chain read for `key`, with its initial state.
    pub fn of(chain: Chain) -> Self {
        let enabled = state(&chain, &|_| None);
        Self { chain, enabled }
    }
    /// The state after `set` (a record's own last Enable or Disable).
    pub fn with(&self, set: &dyn Fn(&FormKey) -> Option<bool>) -> Option<bool> {
        state(&self.chain, set)
    }
    /// [`Presence::with`] for a chain the rules leave open only because a
    /// record below its root carries its own flag 0x800
    /// (`xesp_child_flag_agrees_chain_v1`): decided when every such flag
    /// agrees with the start state the chain gives that record (the flag
    /// set where the chain starts it disabled), the chain then read as any
    /// other. `None` when another part is open or a flag disagrees.
    pub fn with_agreeing_child_flags(
        &self,
        set: &dyn Fn(&FormKey) -> Option<bool>,
    ) -> Option<bool> {
        let open = findings(&self.chain)
            .into_iter()
            .filter(|f| rule(f).is_none())
            .any(|f| f != FOUND_OWN_FLAG);
        if open || !child_flags_agree(&self.chain) {
            return None;
        }
        walk(&self.chain, set)
    }
    /// Whether the chain names `key` above the record itself.
    pub fn depends_on(&self, key: &FormKey) -> bool {
        self.chain.nodes.iter().skip(1).any(|n| n.key == *key)
    }
}

/// The `found` texts a chain raises, the first undecided one included.
pub fn findings(chain: &Chain) -> Vec<&'static str> {
    let mut out = vec![FOUND_FOLLOWS, FOUND_INITIAL];
    let bits = chain
        .nodes
        .iter()
        .filter_map(|n| n.link.as_ref())
        .fold(0u8, |a, l| a | l.flags);
    if bits & 1 != 0 {
        out.push(FOUND_OPPOSITE);
    }
    if bits & 2 != 0 {
        out.push(FOUND_BIT1);
    }
    if bits & !3 != 0 {
        out.push(FOUND_OTHER_BITS);
    }
    match chain.end {
        ChainEnd::Root => {}
        ChainEnd::Cycle(_) => out.push(FOUND_CYCLE),
        ChainEnd::TooDeep => out.push(FOUND_TOO_DEEP),
        ChainEnd::NullParent | ChainEnd::UnresolvedParent { .. } | ChainEnd::MissingParent(_) => {
            out.push(FOUND_UNRESOLVED)
        }
    }
    let n = chain.nodes.len();
    if chain
        .nodes
        .iter()
        .enumerate()
        .any(|(k, x)| k + 1 < n && x.initially_disabled)
    {
        out.push(FOUND_OWN_FLAG);
    }
    out
}

/// The covering rule of a `RuntimeEnableParent` blocker's `found` text.
pub fn rule(found: &str) -> Option<&'static str> {
    match found {
        FOUND_FOLLOWS => Some(hypotheses::ENABLE_FOLLOWS),
        FOUND_INITIAL => Some(hypotheses::ENABLE_INITIAL),
        FOUND_OPPOSITE => Some(hypotheses::ENABLE_OPPOSITE),
        FOUND_BIT1 => Some(hypotheses::ENABLE_BIT1),
        _ => None,
    }
}

/// Pushes the chain's blockers on `blockers`.
pub fn blockers(
    chain: &Chain,
    src: &SourceSpan,
    blockers: &mut Vec<Error>,
    b: &Budget,
) -> Result<()> {
    for f in findings(chain) {
        b.reserve(blockers, 1, &src.file, src.offset)?;
        blockers.push(Error::new(
            "RuntimeEnableParent",
            &src.file,
            src.offset,
            "presence decided by registered rules",
            f,
        ));
    }
    Ok(())
}

/// The state of `chain.nodes[0]`: the root starts as its header flag says
/// (or as `set` says), each record below it as its parent, inverted by flag
/// bit 0, unless `set` names it. `None` when the chain is undecided.
fn state(chain: &Chain, set: &dyn Fn(&FormKey) -> Option<bool>) -> Option<bool> {
    let f = findings(chain);
    if f.iter().any(|x| rule(x).is_none()) {
        return None;
    }
    walk(chain, set)
}

/// Whether every record below the root that carries its own flag 0x800
/// starts disabled through its chain (the root as its flag says, each link
/// as its parent, inverted by flag bit 0).
pub fn child_flags_agree(chain: &Chain) -> bool {
    let n = chain.nodes.len();
    let Some(root) = chain.nodes.last() else {
        return false;
    };
    let mut on = !root.initially_disabled;
    for (k, x) in chain.nodes.iter().enumerate().rev().skip(1) {
        let opposite = x.link.as_ref().is_some_and(|l| l.flags & 1 != 0);
        on = on != opposite;
        if k + 1 < n && x.initially_disabled && on {
            return false;
        }
    }
    true
}

/// The state of `chain.nodes[0]` down from its root, with no check of the
/// findings (the caller's).
fn walk(chain: &Chain, set: &dyn Fn(&FormKey) -> Option<bool>) -> Option<bool> {
    let mut nodes = chain.nodes.iter().rev();
    let root = nodes.next()?;
    let mut on = set(&root.key).unwrap_or(!root.initially_disabled);
    for n in nodes {
        let opposite = n.link.as_ref().is_some_and(|l| l.flags & 1 != 0);
        on = set(&n.key).unwrap_or(on != opposite);
    }
    Some(on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_plugin::enable::{EnableLink, Node};

    fn key(id: u32) -> FormKey {
        FormKey {
            plugin: "t.esm".into(),
            object_id: id,
        }
    }
    fn node(id: u32, disabled: bool, parent: Option<(u32, u8)>) -> Node {
        Node {
            key: key(id),
            signature: *b"REFR",
            initially_disabled: disabled,
            deleted: false,
            link: parent.map(|(p, flags)| EnableLink {
                raw: p,
                parent: Some(key(p)),
                flags,
                reserved: [0; 3],
                subrecord: 1,
            }),
        }
    }

    #[test]
    fn a_child_follows_its_chain_with_the_opposite_flag() {
        // child -> parent (enabled root)
        let c = Chain {
            nodes: vec![node(1, false, Some((2, 0))), node(2, false, None)],
            end: ChainEnd::Root,
        };
        assert_eq!(Presence::of(c.clone()).enabled, Some(true));
        assert_eq!(findings(&c), [FOUND_FOLLOWS, FOUND_INITIAL]);
        // Disabled root.
        let c = Chain {
            nodes: vec![node(1, false, Some((2, 0))), node(2, true, None)],
            end: ChainEnd::Root,
        };
        let p = Presence::of(c);
        assert_eq!(p.enabled, Some(false));
        assert!(p.depends_on(&key(2)) && !p.depends_on(&key(1)));
        // The root enabled by a script: the child follows.
        assert_eq!(p.with(&|k| (*k == key(2)).then_some(true)), Some(true));
        // Two links, the upper one opposite: disabled root -> enabled middle -> enabled child.
        let c = Chain {
            nodes: vec![
                node(1, false, Some((2, 0))),
                node(2, false, Some((3, 1))),
                node(3, true, None),
            ],
            end: ChainEnd::Root,
        };
        assert!(findings(&c).contains(&FOUND_OPPOSITE));
        let p = Presence::of(c);
        assert_eq!(p.enabled, Some(true));
        // The middle disabled by a script wins over its parent.
        assert_eq!(p.with(&|k| (*k == key(2)).then_some(false)), Some(false));
        // Bit 1 needs its rule but does not change the state.
        let c = Chain {
            nodes: vec![node(1, false, Some((2, 2))), node(2, false, None)],
            end: ChainEnd::Root,
        };
        assert!(findings(&c).contains(&FOUND_BIT1));
        assert_eq!(Presence::of(c).enabled, Some(true));
    }

    #[test]
    fn undecided_chains_raise_texts_no_rule_covers() {
        for (c, text) in [
            (
                Chain {
                    nodes: vec![node(1, false, Some((2, 0))), node(2, false, Some((1, 0)))],
                    end: ChainEnd::Cycle(key(1)),
                },
                FOUND_CYCLE,
            ),
            (
                Chain {
                    nodes: vec![node(1, false, Some((9, 0)))],
                    end: ChainEnd::MissingParent(key(9)),
                },
                FOUND_UNRESOLVED,
            ),
            (
                Chain {
                    nodes: vec![node(1, false, Some((2, 4))), node(2, false, None)],
                    end: ChainEnd::Root,
                },
                FOUND_OTHER_BITS,
            ),
            (
                Chain {
                    nodes: vec![
                        node(1, false, Some((2, 0))),
                        node(2, true, Some((3, 0))),
                        node(3, false, None),
                    ],
                    end: ChainEnd::Root,
                },
                FOUND_OWN_FLAG,
            ),
        ] {
            assert!(findings(&c).contains(&text), "{text}");
            assert!(rule(text).is_none());
            assert_eq!(Presence::of(c).enabled, None, "{text}");
        }
    }

    #[test]
    fn a_child_flag_that_agrees_with_its_chain_is_read_as_any_other_link() {
        // The opening's class: the child carries the flag, its link is
        // opposite, the root lacks the flag: both start it disabled.
        let c = Chain {
            nodes: vec![node(1, true, Some((2, 1))), node(2, false, None)],
            end: ChainEnd::Root,
        };
        assert!(findings(&c).contains(&FOUND_OWN_FLAG));
        assert!(child_flags_agree(&c));
        let p = Presence::of(c);
        assert_eq!(p.enabled, None);
        assert_eq!(p.with_agreeing_child_flags(&|_| None), Some(false));
        // The run disables the root: through the opposite link the child
        // is present; its own change still wins.
        let off = |k: &FormKey| (*k == key(2)).then_some(false);
        assert_eq!(p.with_agreeing_child_flags(&off), Some(true));
        assert_eq!(p.with(&off), None);
        let own = |k: &FormKey| match k.object_id {
            1 => Some(false),
            2 => Some(false),
            _ => None,
        };
        assert_eq!(p.with_agreeing_child_flags(&own), Some(false));
        // A flag the chain contradicts (the chain starts it enabled) stays
        // open.
        let c = Chain {
            nodes: vec![node(1, true, Some((2, 0))), node(2, false, None)],
            end: ChainEnd::Root,
        };
        assert!(!child_flags_agree(&c));
        assert_eq!(Presence::of(c).with_agreeing_child_flags(&|_| None), None);
        // An intermediate record's agreeing flag: disabled root, plain
        // links.
        let c = Chain {
            nodes: vec![
                node(1, false, Some((2, 0))),
                node(2, true, Some((3, 0))),
                node(3, true, None),
            ],
            end: ChainEnd::Root,
        };
        assert!(child_flags_agree(&c));
        assert_eq!(
            Presence::of(c).with_agreeing_child_flags(&|_| None),
            Some(false)
        );
        // Another open part keeps it open.
        let c = Chain {
            nodes: vec![node(1, true, Some((2, 5))), node(2, false, None)],
            end: ChainEnd::Root,
        };
        assert_eq!(Presence::of(c).with_agreeing_child_flags(&|_| None), None);
    }
}
