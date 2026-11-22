use crate::tests::{build, name_offset, read, Spec};
use crate::*;
use fo4_core::Budget;

const NAMES: [&str; 8] = [
    "hkClass",
    BEHAVIOR_GRAPH,
    GRAPH_DATA,
    GRAPH_STRINGS,
    STATE_MACHINE,
    STATE_INFO,
    TRANSITIONS,
    SEQUENCE_GENERATOR,
];
/// Object offsets: graph, graph data, string data, state machine, two
/// states, two transition arrays, two sequence generators.
const AT: [usize; 10] = [0, 208, 320, 400, 656, 784, 912, 1024, 1136, 1296];
const CLASS: [usize; 10] = [1, 2, 3, 4, 5, 5, 6, 6, 7, 7];
const END: usize = 1456;

struct Lay {
    bytes: Vec<u8>,
    pointers: Vec<[u32; 2]>,
    links: Vec<[u32; 3]>,
}
impl Lay {
    fn put(&mut self, at: usize, v: u32) {
        self.bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn array(&mut self, at: usize, count: u32, storage: usize) {
        self.put(at + 8, count);
        self.put(at + 12, ARRAY_FLAG | count);
        if count > 0 {
            self.pointers.push([at as u32, storage as u32]);
        }
    }
    fn text(&mut self, source: usize, at: usize, s: &str) {
        self.bytes[at..at + s.len()].copy_from_slice(s.as_bytes());
        self.pointers.push([source as u32, at as u32]);
    }
    fn link(&mut self, source: usize, object: usize) {
        self.links.push([source as u32, 2, AT[object] as u32]);
    }
}

fn file(start: u32, root_class: usize) -> Vec<u8> {
    let names: Vec<(&'static str, u32)> = NAMES.iter().map(|n| (*n, 7)).collect();
    let mut l = Lay {
        bytes: vec![0u8; END],
        pointers: Vec::new(),
        links: Vec::new(),
    };
    l.link(192, 3);
    l.link(200, 1);
    l.link(AT[1] + 104, 2);
    let s = AT[2];
    l.array(s + 16, 2, s + 32);
    l.text(s + 32, s + 48, "Go");
    l.text(s + 40, s + 56, "Back");
    let m = AT[3];
    l.put(m + 168, start);
    l.array(m + 208, 2, m + 240);
    l.link(m + 240, 4);
    l.link(m + 248, 5);
    for (k, (state, trans, seq, name)) in [(4, 6, 8, "A"), (5, 7, 9, "B")].into_iter().enumerate() {
        let a = AT[state];
        l.link(a + 80, trans);
        l.link(a + 88, seq);
        l.text(a + 96, a + 112, name);
        l.put(a + 104, k as u32);
        let t = AT[trans];
        l.array(t + 16, 1, t + 32);
        l.put(t + 32 + 48, k as u32);
        l.put(t + 32 + 52, 1 - k as u32);
        l.put(t + 32 + 64, 0x0800_0000);
        let g = AT[seq];
        l.text(g + 136, g + 144, if k == 0 { "SeqA" } else { "SeqB" });
    }
    let objects = (0..AT.len())
        .map(|k| {
            let class = if k == 3 { root_class } else { CLASS[k] };
            [AT[k] as u32, 0, name_offset(&names, class)]
        })
        .collect();
    build(&Spec {
        names: names.clone(),
        object_bytes: END,
        content: vec![(0, l.bytes)],
        pointers: l.pointers,
        links: l.links,
        objects,
        root: [2, 0, 0, name_offset(&names, 1)],
    })
}

#[test]
fn reads_a_two_state_machine() {
    let bytes = file(1, 4);
    let c = read(&bytes).unwrap();
    let b = Budget::new(1 << 20);
    let m = Machine::read(&c, "synthetic.hkx", 1000, &b).unwrap();
    assert_eq!(m.start, 1);
    assert_eq!(m.events, ["Go", "Back"]);
    assert!(m.wildcards.is_empty());
    let names: Vec<_> = m.states.iter().map(|s| (s.id, s.name.as_str())).collect();
    assert_eq!(names, [(0, "A"), (1, "B")]);
    assert_eq!(m.state(0).unwrap().sequence.as_deref(), Some("SeqA"));
    assert_eq!(m.state(1).unwrap().generator, SEQUENCE_GENERATOR);
    let t = m.state(0).unwrap().transitions[0];
    assert_eq!((t.event, t.to_state, t.condition), (0, 1, None));
    assert_eq!(t.words, [0, 0, 0x0800_0000, 0]);
    assert_eq!(only_object(&c, STATE_INFO), None);
    assert_eq!(only_object(&c, STATE_MACHINE), Some(3));
}

#[test]
fn refuses_a_root_that_is_not_a_state_machine() {
    // The root link names an object of another class (a state info).
    let bytes = file(0, 5);
    let c = read(&bytes).unwrap();
    let e = Machine::read(&c, "synthetic.hkx", 1000, &Budget::new(1 << 20)).unwrap_err();
    assert_eq!(e.code, "UnsupportedBehaviorRoot");
}
