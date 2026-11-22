//! Story manager records (SMEN event nodes, SMBN branch nodes, SMQN quest
//! nodes) read to the last byte, and the node tree they form
//! (`spec/story-manager.md`).
//!
//! No public page of the layout could be read at this step (the UESP file
//! format pages answered HTTP 403 on 2026-11-22): the order grammar, the
//! sizes and the links below come from measurement alone, over every
//! winning node of one installation (`story_census`,
//! `spec/story-manager-census.txt`). The tags' roles were recognised from
//! prior general knowledge and are kept only where that census holds:
//! PNAM names the parent node (an SMEN or SMBN, a word below 0x800 for an
//! SMEN), SNAM the previous sibling or zero, CITC the length of the
//! condition list that follows it, NNAM a QUST and QNAM their count, ENAM a
//! four-byte code that is also stored in the QUST ENAM of the quests listed
//! under the event. DNAM, XNAM, MNAM, HNAM and RNAM are kept as stored
//! words under a named reason ([`UNSETTLED`]): no meaning of theirs is
//! read here. What a node does in the running game is behaviour, held as
//! pending `sm_*` rules by the code that relies on it (`fo4_vm::story`).
use crate::{
    actor::Context,
    condition::{Condition, Ctda},
    typed::{FormRef, Store},
    FormKey, Record,
};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// The three node record types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Kind {
    /// SMEN: the root of one event's tree.
    Event,
    /// SMBN.
    Branch,
    /// SMQN: lists quests.
    Quest,
}
impl Kind {
    pub fn of(sig: [u8; 4]) -> Option<Kind> {
        match &sig {
            b"SMEN" => Some(Kind::Event),
            b"SMBN" => Some(Kind::Branch),
            b"SMQN" => Some(Kind::Quest),
            _ => None,
        }
    }
    pub fn signature(self) -> [u8; 4] {
        match self {
            Kind::Event => *b"SMEN",
            Kind::Branch => *b"SMBN",
            Kind::Quest => *b"SMQN",
        }
    }
}

/// Subrecords kept as stored words, with the reason no meaning is read.
pub const UNSETTLED: [([u8; 4], &str); 5] = [
    (
        *b"DNAM",
        "flag word: two u16 halves counted, no bit given a meaning",
    ),
    (*b"XNAM", "u32 counted, no meaning read"),
    (*b"MNAM", "u32 counted, no meaning read"),
    (
        *b"HNAM",
        "u32 that reads as a finite f32 in every measured node; no meaning read",
    ),
    (
        *b"RNAM",
        "u32 after a quest that reads as a finite f32 in every measured case; no meaning read",
    ),
];

/// The reason a tag is kept as stored, when [`UNSETTLED`] names it.
pub fn unsettled(tag: [u8; 4]) -> Option<&'static str> {
    UNSETTLED.iter().find(|(t, _)| *t == tag).map(|(_, r)| *r)
}

/// One subrecord in stored order and whether it is given a field (typed)
/// or kept as a stored word under a reason.
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub index: usize,
    pub tag: [u8; 4],
    pub size: usize,
    pub unsettled: Option<&'static str>,
}

/// One quest of a quest node: NNAM and the RNAM after it, if any.
#[derive(Debug, Clone, Serialize)]
pub struct QuestEntry {
    pub quest: FormRef,
    /// RNAM, kept as stored.
    pub rnam: Option<u32>,
}

/// One node record, every subrecord placed.
#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub kind: Kind,
    /// The load-order index of the plugin the record was read from (the
    /// master context of its FormIDs and conditions).
    pub plugin: usize,
    /// The file the record was read from.
    pub file: String,
    /// EDID, the bytes before the NUL.
    pub editor_id: Option<Vec<u8>>,
    /// PNAM: the parent node.
    pub parent: FormRef,
    /// SNAM: the previous sibling (zero for none).
    pub previous: FormRef,
    /// CITC, as stored.
    pub condition_count: u32,
    pub conditions: Vec<Condition>,
    /// DNAM, as stored.
    pub dnam: u32,
    /// XNAM, as stored.
    pub xnam: u32,
    /// ENAM of an event node: four bytes as stored.
    pub event: Option<[u8; 4]>,
    /// MNAM of a quest node, as stored.
    pub mnam: Option<u32>,
    /// HNAM of a quest node, as stored.
    pub hnam: Option<u32>,
    /// QNAM of a quest node, as stored.
    pub quest_count: Option<u32>,
    pub quests: Vec<QuestEntry>,
    pub fields: Vec<Field>,
}

struct Reader<'a> {
    r: &'a Record,
    ctx: Context<'a>,
    b: &'a Budget,
    i: usize,
    fields: Vec<Field>,
}
impl<'a> Reader<'a> {
    fn file(&self) -> &str {
        &self.r.source.file
    }
    fn offset_at(&self, i: usize) -> u64 {
        self.r
            .subrecords
            .get(i)
            .map_or(self.r.source.offset, |s| s.offset)
    }
    fn peek(&self) -> Option<[u8; 4]> {
        self.r.subrecords.get(self.i).map(|s| s.signature)
    }
    fn at(&self, tag: &[u8; 4]) -> bool {
        self.peek() == Some(*tag)
    }
    fn order(&self, expected: &str) -> Error {
        let found = match self.peek() {
            Some(t) => String::from_utf8_lossy(&t).into_owned(),
            None => "the end of the record".into(),
        };
        Error::new(
            "StoryNodeOrder",
            self.file(),
            self.offset_at(self.i),
            expected,
            found,
        )
    }
    fn push<T>(&self, v: &mut Vec<T>, x: T) -> Result<()> {
        self.b.reserve(v, 1, self.file(), self.offset_at(self.i))?;
        v.push(x);
        Ok(())
    }
    fn take(&mut self, tag: &[u8; 4], expected: &str) -> Result<(usize, &'a [u8])> {
        if !self.at(tag) {
            return Err(self.order(expected));
        }
        let i = self.i;
        let r: &'a Record = self.r;
        let d = r.data(&r.subrecords[i]);
        let f = Field {
            index: i,
            tag: *tag,
            size: d.len(),
            unsettled: unsettled(*tag),
        };
        let mut fields = std::mem::take(&mut self.fields);
        self.push(&mut fields, f)?;
        self.fields = fields;
        self.i += 1;
        Ok((i, d))
    }
    fn word(&mut self, tag: &[u8; 4], expected: &str) -> Result<(usize, u32)> {
        let (i, d) = self.take(tag, expected)?;
        let d: [u8; 4] = d.try_into().map_err(|_| {
            Error::new(
                "StoryFieldSize",
                self.file(),
                self.offset_at(i),
                format!("{} of 4 bytes", String::from_utf8_lossy(tag)),
                d.len(),
            )
        })?;
        Ok((i, u32::from_le_bytes(d)))
    }
    fn form(&mut self, tag: &[u8; 4], expected: &str) -> Result<FormRef> {
        let (i, raw) = self.word(tag, expected)?;
        Ok(FormRef {
            raw,
            key: self
                .ctx
                .order
                .remap(self.ctx.plugin, raw, self.file(), self.offset_at(i))?,
            subrecord: i,
        })
    }
    fn text(&mut self, tag: &[u8; 4]) -> Result<Vec<u8>> {
        let (i, d) = self.take(tag, "text")?;
        match d.split_last() {
            Some((0, t)) if !t.contains(&0) => {
                self.b
                    .charge(t.len() as u64, self.file(), self.offset_at(i))?;
                Ok(t.to_vec())
            }
            _ => Err(Error::new(
                "StoryFieldText",
                self.file(),
                self.offset_at(i),
                format!(
                    "{} of bytes ending with one NUL",
                    String::from_utf8_lossy(tag)
                ),
                d.len(),
            )),
        }
    }
    fn condition(&mut self) -> Result<Condition> {
        let (i, d) = self.take(b"CTDA", "CTDA")?;
        let ctda = Ctda::read(d, &self.r.source.file, self.offset_at(i))?;
        let cis1 = if self.at(b"CIS1") {
            Some(self.text(b"CIS1")?)
        } else {
            None
        };
        let cis2 = if self.at(b"CIS2") {
            Some(self.text(b"CIS2")?)
        } else {
            None
        };
        Ok(Condition {
            index: i,
            ctda,
            cis1,
            cis2,
        })
    }
}

impl Node {
    /// Reads an SMEN, SMBN or SMQN record in the master context `ctx`. The
    /// order (every part required unless in brackets):
    ///
    /// ```text
    /// [EDID] PNAM SNAM CITC (CTDA [CIS1] [CIS2])* DNAM XNAM
    ///   SMEN: ENAM
    ///   SMQN: [MNAM] [HNAM] QNAM (NNAM [RNAM])*
    /// ```
    ///
    /// A record of another type is `StoryNodeType`; a subrecord out of
    /// this order `StoryNodeOrder`; a word not of 4 bytes `StoryFieldSize`;
    /// a text without one closing NUL `StoryFieldText`.
    pub fn read(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Node> {
        let kind = Kind::of(r.signature).ok_or_else(|| {
            Error::new(
                "StoryNodeType",
                &r.source.file,
                r.source.offset,
                "SMEN, SMBN or SMQN",
                String::from_utf8_lossy(&r.signature),
            )
        })?;
        let mut x = Reader {
            r,
            ctx,
            b,
            i: 0,
            fields: Vec::new(),
        };
        let editor_id = if x.at(b"EDID") {
            Some(x.text(b"EDID")?)
        } else {
            None
        };
        let parent = x.form(b"PNAM", "PNAM")?;
        let previous = x.form(b"SNAM", "SNAM")?;
        let (_, condition_count) = x.word(b"CITC", "CITC")?;
        let mut conditions = Vec::new();
        while x.at(b"CTDA") {
            let c = x.condition()?;
            x.push(&mut conditions, c)?;
        }
        if x.at(b"CIS1") || x.at(b"CIS2") {
            return Err(x.order("CTDA before CIS1 or CIS2, or DNAM"));
        }
        let (_, dnam) = x.word(b"DNAM", "DNAM")?;
        let (_, xnam) = x.word(b"XNAM", "XNAM")?;
        let mut node = Node {
            kind,
            plugin: ctx.plugin,
            file: r.source.file.clone(),
            editor_id,
            parent,
            previous,
            condition_count,
            conditions,
            dnam,
            xnam,
            event: None,
            mnam: None,
            hnam: None,
            quest_count: None,
            quests: Vec::new(),
            fields: Vec::new(),
        };
        match kind {
            Kind::Event => {
                let (_, w) = x.word(b"ENAM", "ENAM")?;
                node.event = Some(w.to_le_bytes());
            }
            Kind::Branch => {}
            Kind::Quest => {
                if x.at(b"MNAM") {
                    node.mnam = Some(x.word(b"MNAM", "MNAM")?.1);
                }
                if x.at(b"HNAM") {
                    node.hnam = Some(x.word(b"HNAM", "HNAM")?.1);
                }
                node.quest_count = Some(x.word(b"QNAM", "MNAM, HNAM or QNAM")?.1);
                while x.at(b"NNAM") {
                    let quest = x.form(b"NNAM", "NNAM")?;
                    let rnam = if x.at(b"RNAM") {
                        Some(x.word(b"RNAM", "RNAM")?.1)
                    } else {
                        None
                    };
                    let mut q = std::mem::take(&mut node.quests);
                    x.push(&mut q, QuestEntry { quest, rnam })?;
                    node.quests = q;
                }
            }
        }
        if x.i != r.subrecords.len() {
            return Err(x.order("the end of the record"));
        }
        node.fields = x.fields;
        Ok(node)
    }
    /// The quests the node lists, resolved.
    pub fn quest_keys(&self) -> impl Iterator<Item = &FormKey> {
        self.quests.iter().filter_map(|q| q.quest.key.as_ref())
    }
}

/// How the previous-sibling links order the children of one parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Order {
    /// One child has no previous sibling among them, each other names
    /// exactly one, and no two name the same: one chain.
    Chain,
    /// Anything else: the children are kept in key order and the order is
    /// not settled by the links.
    Ambiguous,
}

/// Every winning node and the tree the parent and previous links make.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub nodes: BTreeMap<FormKey, Node>,
    /// Children of each parent key, in previous-link order when it is a
    /// chain, else in key order.
    pub children: BTreeMap<FormKey, (Vec<FormKey>, Order)>,
    /// Nodes that read with an error, with its text.
    pub errors: Vec<(FormKey, String)>,
}

/// Deepest node walk followed (parent links) before giving up.
pub const MAX_DEPTH: usize = 64;

impl Tree {
    /// Reads every winning SMEN, SMBN and SMQN of the store.
    pub fn read(store: &Store, b: &Budget) -> Result<Tree> {
        let keys: Vec<FormKey> = store
            .winners()
            .filter(|w| !w.deleted && Kind::of(w.signature).is_some())
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect();
        let mut nodes = BTreeMap::new();
        let mut errors = Vec::new();
        for k in keys {
            let work = b.child();
            let Some((r, plugin)) = store.raw_with_plugin(&k, &work)? else {
                continue;
            };
            let ctx = Context {
                order: store.order(),
                plugin,
            };
            let keep = b.child();
            match Node::read(&r, ctx, &keep) {
                Ok(n) => {
                    keep.commit(b)?;
                    nodes.insert(k, n);
                }
                Err(e) => errors.push((k, e.to_string())),
            }
        }
        Ok(Tree::from_nodes(nodes, errors))
    }
    /// The tree of nodes already read.
    pub fn from_nodes(nodes: BTreeMap<FormKey, Node>, errors: Vec<(FormKey, String)>) -> Tree {
        let mut by_parent: BTreeMap<FormKey, Vec<FormKey>> = BTreeMap::new();
        for (k, n) in &nodes {
            if let Some(p) = &n.parent.key {
                by_parent.entry(p.clone()).or_default().push(k.clone());
            }
        }
        let mut children = BTreeMap::new();
        for (p, kids) in by_parent {
            let set: BTreeSet<&FormKey> = kids.iter().collect();
            let prev = |k: &FormKey| -> Option<&FormKey> {
                nodes[k].previous.key.as_ref().filter(|x| set.contains(x))
            };
            let heads: Vec<&FormKey> = kids.iter().filter(|k| prev(k).is_none()).collect();
            let mut followers: BTreeMap<&FormKey, Vec<&FormKey>> = BTreeMap::new();
            for k in &kids {
                if let Some(x) = prev(k) {
                    followers.entry(x).or_default().push(k);
                }
            }
            let mut chain = Vec::new();
            let mut ok = heads.len() == 1 && followers.values().all(|f| f.len() == 1);
            if ok {
                let mut at = heads[0];
                loop {
                    if chain.contains(at) {
                        ok = false;
                        break;
                    }
                    chain.push(at.clone());
                    match followers.get(at) {
                        Some(f) => at = f[0],
                        None => break,
                    }
                }
                ok = ok && chain.len() == kids.len();
            }
            let entry = if ok {
                (chain, Order::Chain)
            } else {
                (kids, Order::Ambiguous)
            };
            children.insert(p, entry);
        }
        Tree {
            nodes,
            children,
            errors,
        }
    }
    /// The children of a node (empty for none).
    pub fn children_of(&self, k: &FormKey) -> (&[FormKey], Order) {
        match self.children.get(k) {
            Some((c, o)) => (c, o.clone()),
            None => (&[], Order::Chain),
        }
    }
    /// The path from a node up to its event node, the node first; `None`
    /// when a parent link leaves the nodes before an event node, loops, or
    /// is longer than [`MAX_DEPTH`].
    pub fn path(&self, k: &FormKey) -> Option<Vec<&FormKey>> {
        let mut out: Vec<&FormKey> = Vec::new();
        let (mut at, mut n) = self.nodes.get_key_value(k)?;
        loop {
            if out.contains(&at) || out.len() > MAX_DEPTH {
                return None;
            }
            out.push(at);
            if n.kind == Kind::Event {
                return Some(out);
            }
            let p = n.parent.key.as_ref()?;
            (at, n) = self.nodes.get_key_value(p)?;
        }
    }
    /// The event code of the tree a node belongs to.
    pub fn event_of(&self, k: &FormKey) -> Option<[u8; 4]> {
        let p = self.path(k)?;
        self.nodes[*p.last()?].event
    }
    /// Every event node's code with the quests its tree lists (distinct).
    pub fn quests_by_event(&self) -> BTreeMap<[u8; 4], BTreeSet<FormKey>> {
        let mut out: BTreeMap<[u8; 4], BTreeSet<FormKey>> = BTreeMap::new();
        for (k, n) in &self.nodes {
            if n.kind == Kind::Event {
                if let Some(e) = n.event {
                    out.entry(e).or_default();
                }
            }
            if n.kind != Kind::Quest {
                continue;
            }
            if let Some(e) = self.event_of(k) {
                out.entry(e).or_default().extend(n.quest_keys().cloned());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Header, LoadOrder, Subrecord};
    use fo4_core::{Limits, SliceSource, SourceSpan};

    fn order() -> LoadOrder {
        let mut hedr = 0.95f32.to_le_bytes().to_vec();
        hedr.extend(0u32.to_le_bytes());
        hedr.extend(0x900u32.to_le_bytes());
        let mut data = b"HEDR".to_vec();
        data.extend((hedr.len() as u16).to_le_bytes());
        data.extend(hedr);
        let mut bytes = b"TES4".to_vec();
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend([0; 8]);
        bytes.extend(131u16.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(data);
        let h = Header::read(
            "t.esm",
            &SliceSource {
                name: "t.esm",
                bytes: &bytes,
            },
            &Limits::default(),
            &Budget::new(1 << 20),
        )
        .unwrap();
        LoadOrder::new(vec![h], &Budget::new(1 << 20)).unwrap()
    }
    fn record(sig: &[u8; 4], subs: &[(&[u8; 4], Vec<u8>)]) -> Record {
        let mut payload = Vec::new();
        let mut subrecords = Vec::new();
        for (i, (t, d)) in subs.iter().enumerate() {
            let start = payload.len();
            payload.extend_from_slice(d);
            subrecords.push(Subrecord {
                signature: **t,
                data: start..payload.len(),
                offset: 100 + i as u64,
                extended: false,
            });
        }
        Record {
            signature: *sig,
            flags: 0,
            form_id: 0x800,
            revision: 0,
            version: 131,
            unknown: 0,
            source: SourceSpan::new("t.esm", 0, 0),
            payload,
            subrecords,
            payload_is_decompressed: false,
        }
    }
    fn w(v: u32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }
    fn ctda(function: u16) -> Vec<u8> {
        let mut d = vec![0u8; 32];
        d[8..10].copy_from_slice(&function.to_le_bytes());
        d
    }

    #[test]
    fn nodes_read_to_the_last_byte_in_their_order() {
        let order = order();
        let ctx = Context {
            order: &order,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let ev = record(
            b"SMEN",
            &[
                (b"PNAM", w(0x5b)),
                (b"SNAM", w(0)),
                (b"CITC", w(0)),
                (b"DNAM", w(0)),
                (b"XNAM", w(0)),
                (b"ENAM", b"SCPT".to_vec()),
            ],
        );
        let n = Node::read(&ev, ctx, &b).unwrap();
        assert_eq!(n.kind, Kind::Event);
        assert_eq!(n.event, Some(*b"SCPT"));
        assert_eq!(n.fields.len(), 6);
        assert_eq!(n.fields[3].unsettled, unsettled(*b"DNAM"));
        let q = record(
            b"SMQN",
            &[
                (b"EDID", b"N\0".to_vec()),
                (b"PNAM", w(0x20)),
                (b"SNAM", w(0x21)),
                (b"CITC", w(1)),
                (b"CTDA", ctda(576)),
                (b"CIS2", b"x\0".to_vec()),
                (b"DNAM", w(0x0001_0001)),
                (b"XNAM", w(0)),
                (b"HNAM", w(0x4290_0000)),
                (b"QNAM", w(2)),
                (b"NNAM", w(0x30)),
                (b"RNAM", w(0x4410_0000)),
                (b"NNAM", w(0x31)),
            ],
        );
        let n = Node::read(&q, ctx, &b).unwrap();
        assert_eq!(n.conditions.len(), 1);
        assert_eq!(n.conditions[0].ctda.function, 576);
        assert_eq!(n.conditions[0].cis2.as_deref(), Some(&b"x"[..]));
        assert_eq!(n.quest_count, Some(2));
        assert_eq!(n.quests.len(), 2);
        assert_eq!(n.quests[0].rnam, Some(0x4410_0000));
        assert_eq!(n.quests[1].rnam, None);
        assert_eq!(n.hnam, Some(0x4290_0000));
        // Out of order: XNAM before DNAM.
        let bad = record(
            b"SMBN",
            &[
                (b"PNAM", w(0x20)),
                (b"SNAM", w(0)),
                (b"CITC", w(0)),
                (b"XNAM", w(0)),
                (b"DNAM", w(0)),
            ],
        );
        let e = Node::read(&bad, ctx, &b).unwrap_err().to_string();
        assert!(e.contains("StoryNodeOrder"), "{e}");
        // A word of the wrong size.
        let bad = record(
            b"SMBN",
            &[
                (b"PNAM", vec![0; 3]),
                (b"SNAM", w(0)),
                (b"CITC", w(0)),
                (b"DNAM", w(0)),
                (b"XNAM", w(0)),
            ],
        );
        let e = Node::read(&bad, ctx, &b).unwrap_err().to_string();
        assert!(e.contains("StoryFieldSize"), "{e}");
        // Something after the last part.
        let bad = record(
            b"SMEN",
            &[
                (b"PNAM", w(0x5b)),
                (b"SNAM", w(0)),
                (b"CITC", w(0)),
                (b"DNAM", w(0)),
                (b"XNAM", w(0)),
                (b"ENAM", b"SCPT".to_vec()),
                (b"NNAM", w(1)),
            ],
        );
        assert!(Node::read(&bad, ctx, &b).is_err());
    }

    fn node(kind: Kind, parent: u32, previous: u32) -> Node {
        let k = |id: u32| FormRef {
            raw: id,
            key: (id != 0).then(|| FormKey {
                plugin: "t.esm".into(),
                object_id: id,
            }),
            subrecord: 0,
        };
        Node {
            kind,
            plugin: 0,
            file: "t.esm".into(),
            editor_id: None,
            parent: k(parent),
            previous: k(previous),
            condition_count: 0,
            conditions: Vec::new(),
            dnam: 0,
            xnam: 0,
            event: (kind == Kind::Event).then_some(*b"SCPT"),
            mnam: None,
            hnam: None,
            quest_count: None,
            quests: Vec::new(),
            fields: Vec::new(),
        }
    }
    fn key(id: u32) -> FormKey {
        FormKey {
            plugin: "t.esm".into(),
            object_id: id,
        }
    }

    #[test]
    fn previous_links_order_siblings_or_say_they_do_not() {
        let mut nodes = BTreeMap::new();
        nodes.insert(key(1), node(Kind::Event, 0x5b, 0));
        // Children of 1: 4 <- 2 <- 3 (3 after 2 after 4).
        nodes.insert(key(2), node(Kind::Branch, 1, 4));
        nodes.insert(key(3), node(Kind::Quest, 1, 2));
        nodes.insert(key(4), node(Kind::Quest, 1, 0));
        // Children of 2: two name the same previous sibling.
        nodes.insert(key(5), node(Kind::Quest, 2, 0));
        nodes.insert(key(6), node(Kind::Quest, 2, 5));
        nodes.insert(key(7), node(Kind::Quest, 2, 5));
        let t = Tree::from_nodes(nodes, Vec::new());
        let (c, o) = t.children_of(&key(1));
        assert_eq!(o, Order::Chain);
        assert_eq!(c, [key(4), key(2), key(3)]);
        let (c, o) = t.children_of(&key(2));
        assert_eq!(o, Order::Ambiguous);
        assert_eq!(c.len(), 3);
        assert_eq!(t.path(&key(6)).unwrap().len(), 3);
        assert_eq!(t.event_of(&key(6)), Some(*b"SCPT"));
        assert!(t.children_of(&key(3)).0.is_empty());
    }
}
