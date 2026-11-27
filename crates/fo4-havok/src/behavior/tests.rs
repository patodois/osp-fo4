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

/// A graph whose root machine's start state is bound to an int variable
/// (1), state 1 runs a selector (stored index 1) of a clip and a blender,
/// the blender's second child's weight is bound to a real variable (0.75,
/// above the first child's stored 0.5) and plays the clip `Idle`; the
/// first state plays a reference.
pub(crate) fn node_file() -> Vec<u8> {
    use crate::fixture::Objects;
    let mut o = Objects::new();
    let graph = o.object(BEHAVIOR_GRAPH, 208);
    let data = o.object(GRAPH_DATA, 160);
    let values = o.object(VALUE_SET, 96);
    let strings = o.object(GRAPH_STRINGS, 160);
    let machine = o.object(STATE_MACHINE, 256);
    let s0 = o.object(STATE_INFO, 128);
    let s1 = o.object(STATE_INFO, 128);
    let start_binding = o.object(BINDING_SET, 112);
    let reference = o.object(BEHAVIOR_REFERENCE, 176);
    let selector = o.object(MANUAL_SELECTOR, 224);
    let wrong = o.object(CLIP_GENERATOR, 224);
    let blender = o.object(BLENDER, 192);
    let c0 = o.object(BLENDER_CHILD, 80);
    let c1 = o.object(BLENDER_CHILD, 80);
    let weight_binding = o.object(BINDING_SET, 112);
    let idle = o.object(CLIP_GENERATOR, 224);
    let swap = o.object(SWAP_GENERATOR, 64);
    o.link(graph, 192, machine);
    o.link(graph, 200, data);
    // Two variables: an int (type 3) and a real (type 4).
    o.array(data, 32, 2, 128);
    o.put_u8(data, 128 + 4, 3);
    o.put_u8(data, 134 + 4, 4);
    o.link(data, 96, values);
    o.link(data, 104, strings);
    o.array(values, 16, 2, 64);
    o.array(values, 32, 0, 0);
    o.array(values, 48, 0, 0);
    o.put(values, 64, 1);
    o.put_f32(values, 68, 0.75);
    o.array(strings, 16, 1, 80);
    o.text(strings, 80, 96, "Go");
    o.array(strings, 48, 2, 104);
    o.text(strings, 104, 120, "iState");
    o.text(strings, 112, 136, "fWeight");
    o.put(machine, 168, 0);
    o.link(machine, 16, start_binding);
    o.array(machine, 208, 2, 240);
    o.link(machine, 240, s0);
    o.link(machine, 248, s1);
    for (s, id, g) in [(s0, 0, reference), (s1, 1, selector)] {
        o.link(s, 88, g);
        o.text(s, 96, 112, if id == 0 { "A" } else { "B" });
        o.put(s, 104, id);
    }
    for (set, path, var) in [
        (start_binding, "startStateId", 0),
        (weight_binding, "weight", 1),
    ] {
        o.array(set, 16, 1, 48);
        o.text(set, 48, 88, path);
        o.put(set, 48 + 28, var);
        o.put_u8(set, 48 + 32, 0xff);
    }
    o.text(reference, 136, 144, "Behaviors\\Other.hkx");
    o.array(selector, 136, 2, 192);
    o.link(selector, 192, wrong);
    o.link(selector, 200, blender);
    o.put_u8(selector, 152, 1);
    o.array(blender, 160, 2, 176);
    o.link(blender, 176, c0);
    o.link(blender, 184, c1);
    o.put(blender, 152, 0x0001_ffff);
    o.link(c0, 48, wrong);
    o.put_f32(c0, 64, 0.5);
    o.link(c1, 48, idle);
    o.put_f32(c1, 64, 0.0);
    o.link(c1, 16, weight_binding);
    for (g, name, mode) in [
        (wrong, "Animations\\Wrong.hkt", 0),
        (idle, "Animations\\Idle.hkt", 1),
    ] {
        o.text(g, 144, 192, name);
        o.put_f32(g, 176, 1.0);
        o.put(g, 188, 0x00ff_ffff | (u32::from(mode) << 16));
        o.put_u8(g, 190, mode);
    }
    let _ = swap;
    o.file()
}

#[test]
fn reads_generators_bindings_and_variables() {
    let bytes = node_file();
    let c = read(&bytes).unwrap();
    let b = Budget::new(1 << 20);
    let f = "synthetic.hkx";
    let v = Variables::read(&c, graph_data(&c, f, 0).unwrap(), f, 0, &b).unwrap();
    assert_eq!(v.types, [3, 4]);
    assert_eq!(v.names, ["iState", "fWeight"]);
    assert_eq!(
        (v.number(0), v.number(1), v.number(2)),
        (Some(1.), Some(0.75), None)
    );
    let Node::Machine(m) = Node::read(&c, 4, f, 0, &b).unwrap() else {
        panic!("machine")
    };
    assert_eq!(m.start, 0);
    let bs = bindings(&c, 4, f, 0, &b).unwrap();
    assert_eq!(
        bs,
        [VariableBinding {
            path: "startStateId".into(),
            variable: 0,
            bit: -1,
            kind: 0
        }]
    );
    assert_eq!(
        machine_state(&c, &m, 1, f, 0).unwrap().unwrap().generator,
        Some(9)
    );
    assert!(machine_state(&c, &m, 7, f, 0).unwrap().is_none());
    assert_eq!(
        Node::read(&c, 8, f, 0, &b).unwrap(),
        Node::Reference {
            behavior: "Behaviors\\Other.hkx".into()
        }
    );
    assert_eq!(
        Node::read(&c, 9, f, 0, &b).unwrap(),
        Node::Selector {
            generators: vec![10, 11],
            selected: 1
        }
    );
    let Node::Blender {
        word152,
        flags,
        children,
    } = Node::read(&c, 11, f, 0, &b).unwrap()
    else {
        panic!("blender")
    };
    assert_eq!((word152, flags), (-1, 1));
    assert_eq!(
        children
            .iter()
            .map(|x| (x.generator, x.weight))
            .collect::<Vec<_>>(),
        [(Some(10), 0.5), (Some(15), 0.0)]
    );
    assert_eq!(bindings(&c, 13, f, 0, &b).unwrap()[0].path, "weight");
    let Node::Clip(g) = Node::read(&c, 15, f, 0, &b).unwrap() else {
        panic!("clip")
    };
    assert_eq!(
        (g.clip.as_str(), g.mode, g.binding_index),
        ("Animations\\Idle.hkt", 1, -1)
    );
    assert_eq!((g.playback_speed, g.start_time), (1.0, 0.0));
    assert_eq!(Node::read(&c, 16, f, 0, &b).unwrap(), Node::Swap);
    assert_eq!(
        Node::read(&c, 1, f, 0, &b).unwrap(),
        Node::Unread {
            class: GRAPH_DATA.into()
        }
    );
    assert_eq!(node_name(&c, 15, f, 0), None);
}

#[test]
fn variable_counts_that_disagree_are_named_errors() {
    let mut bytes = node_file();
    let c = read(&bytes).unwrap();
    // The value set's word count (+24 of the value set object) set to 3.
    let at = c.data().unwrap().start as usize + c.objects[2].offset as usize + 24;
    bytes[at..at + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[at + 4..at + 8].copy_from_slice(&0x8000_0003u32.to_le_bytes());
    let c = read(&bytes).unwrap();
    let f = "synthetic.hkx";
    let e = Variables::read(&c, 1, f, 0, &Budget::new(1 << 20)).unwrap_err();
    assert!(
        e.code == "InvalidHavokGraphData" || e.code.starts_with("InvalidHavok"),
        "{}",
        e.code
    );
}

#[test]
fn every_owned_copy_of_a_shared_text_is_charged() {
    // A graph whose event list has 1,000 entries all pointing at one text
    // of 4,000 bytes: 4 MB of owned names from a file of about 12 KB.
    use crate::fixture::Objects;
    let n = 1000usize;
    let text = "e".repeat(4000);
    let mut o = Objects::new();
    let graph = o.object(BEHAVIOR_GRAPH, 208);
    let data = o.object(GRAPH_DATA, 160);
    let at = 32 + 8 * n;
    let strings = o.object(GRAPH_STRINGS, at + text.len() + 1);
    o.link(graph, 192, strings);
    o.link(graph, 200, data);
    o.link(data, 104, strings);
    o.array(strings, 16, n as u32, 32);
    for i in 0..n {
        o.text(strings, 32 + 8 * i, at, &text);
    }
    let bytes = o.file();
    let c = read(&bytes).unwrap();
    // Within a 1 MiB budget the names do not fit: a named limit, not 4 MB
    // allocated past it.
    let e = Machine::read(&c, "synthetic.hkx", 0, &Budget::new(1 << 20)).unwrap_err();
    assert_eq!(e.code, "LimitExceeded");
    // With room for them the reading goes on to the root's class.
    let e = Machine::read(&c, "synthetic.hkx", 0, &Budget::new(64 << 20)).unwrap_err();
    assert_eq!(e.code, "UnsupportedBehaviorRoot");
}

#[test]
fn a_long_table_is_looked_up_without_a_scan_per_entry() {
    // An event list of the most entries a machine is read with, each its
    // own first-table entry: one lookup per element used to scan the
    // whole table.
    use crate::fixture::Objects;
    let n = MAX_ENTRIES as usize;
    let mut o = Objects::new();
    let strings = o.object(GRAPH_STRINGS, 32 + 8 * n + 16);
    o.array(strings, 16, n as u32, 32);
    for i in 0..n {
        o.text(strings, 32 + 8 * i, 32 + 8 * n, "e");
    }
    let bytes = o.file();
    let b = Budget::new(64 << 20);
    let c = Container::read(&bytes, "synthetic.hkx", 0, &b).unwrap();
    let t = std::time::Instant::now();
    let events = graph_events(&c, 0, "synthetic.hkx", 0, &b).unwrap();
    let took = t.elapsed();
    assert_eq!(events.len(), n);
    assert!(events.iter().all(|e| *e == b"e"));
    // A scan per entry is about two billion comparisons; the index takes
    // a few milliseconds.
    assert!(took < std::time::Duration::from_secs(1), "{took:?}");
}
