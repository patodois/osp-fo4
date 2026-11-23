use super::*;
use fo4_core::{Game, Limits, Manifest, Mount, PluginSpec};
use fo4_havok::fixture::Objects;

fn sub(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn rec(tag: &[u8; 4], id: u32, data: &[u8]) -> Vec<u8> {
    let mut v = tag.to_vec();
    for n in [data.len() as u32, 0, id, 0] {
        v.extend(n.to_le_bytes());
    }
    v.extend(131u16.to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v.extend(data);
    v
}
fn id(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn z(s: &str) -> Vec<u8> {
    [s.as_bytes(), &[0]].concat()
}

/// The project names one character, the character the root graph.
fn project() -> Vec<u8> {
    let mut o = Objects::new();
    let p = o.object(hk::PROJECT_STRINGS, 128);
    o.array(p, 48, 1, 80);
    o.text(p, 80, 96, "Characters\\C.hkx");
    o.file()
}
fn character() -> Vec<u8> {
    let mut o = Objects::new();
    let c = o.object(hk::CHARACTER_STRINGS, 240);
    o.text(c, 184, 192, "Behaviors\\Root.hkx");
    o.file()
}

/// A graph object, its data, value set and string data with `vars`
/// variables (type, word), and its root machine; returns the objects
/// and the machine's index.
fn graph(vars: &[(u8, u32)]) -> (Objects, usize) {
    let mut o = Objects::new();
    let g = o.object(hk::BEHAVIOR_GRAPH, 208);
    let d = o.object(hk::GRAPH_DATA, 128 + 16 * vars.len().max(1));
    let v = o.object(hk::VALUE_SET, 64 + 16 * vars.len().max(1));
    let s = o.object(hk::GRAPH_STRINGS, 96 + 32 * vars.len().max(1));
    let m = o.object(hk::STATE_MACHINE, 320);
    o.link(g, 192, m);
    o.link(g, 200, d);
    o.link(d, 96, v);
    o.link(d, 104, s);
    o.array(d, 32, vars.len() as u32, 128);
    o.array(v, 16, vars.len() as u32, 64);
    o.array(v, 32, 0, 0);
    o.array(v, 48, 0, 0);
    o.array(s, 48, vars.len() as u32, 96);
    for (k, (t, w)) in vars.iter().enumerate() {
        o.put_u8(d, 128 + 6 * k + 4, *t);
        o.put(v, 64 + 4 * k, *w);
        o.text(
            s,
            96 + 8 * k,
            96 + 8 * vars.len() + 16 * k,
            &format!("v{k}"),
        );
    }
    (o, m)
}

/// A machine whose states (id, generator) are given, start `start`.
fn states(o: &mut Objects, m: usize, start: i32, list: &[(i32, usize)]) {
    o.put(m, 168, start as u32);
    o.array(m, 208, list.len() as u32, 240);
    for (k, (sid, g)) in list.iter().enumerate() {
        let s = o.object(hk::STATE_INFO, 128);
        o.link(m, 240 + 8 * k, s);
        o.link(s, 88, *g);
        o.text(s, 96, 112, "S");
        o.put(s, 104, *sid as u32);
    }
}

fn clip(o: &mut Objects, name: &str, mode: u8) -> usize {
    let g = o.object(hk::CLIP_GENERATOR, 224);
    o.text(g, 144, 192, name);
    o.put_f32(g, 176, 1.0);
    o.put(g, 188, 0xffff);
    o.put_u8(g, 190, mode);
    g
}

fn binding(o: &mut Objects, node: usize, path: &str, var: u32) {
    let set = o.object(hk::BINDING_SET, 112);
    o.link(node, 16, set);
    o.array(set, 16, 1, 48);
    o.text(set, 48, 88, path);
    o.put(set, 48 + 28, var);
    o.put_u8(set, 48 + 32, 0xff);
}

/// Root: start state 0, a modifier generator around a swap generator.
fn root_graph() -> Vec<u8> {
    let (mut o, m) = graph(&[]);
    let w = o.object(hk::MODIFIER_GENERATOR, 160);
    let swap = o.object(hk::SWAP_GENERATOR, 64);
    o.link(w, 144, swap);
    let other = clip(&mut o, "Animations\\Root.hkt", 0);
    states(&mut o, m, 0, &[(0, w), (1, other)]);
    o.file()
}

/// Subgraph: the start state bound to variable 0 (stored 1); state 1 a
/// blender whose second child's weight is bound to variable 1 (0.75,
/// above the first child's stored 0.5) and plays `Idle` (looping).
fn subgraph() -> Vec<u8> {
    let (mut o, m) = graph(&[(3, 1), (4, 0.75f32.to_bits())]);
    let wrong = clip(&mut o, "Animations\\Wrong.hkt", 0);
    let idle = clip(&mut o, "Animations\\Sub\\Idle.hkt", 1);
    let blender = o.object(hk::BLENDER, 192);
    let c0 = o.object(hk::BLENDER_CHILD, 80);
    let c1 = o.object(hk::BLENDER_CHILD, 80);
    o.array(blender, 160, 2, 176);
    o.link(blender, 176, c0);
    o.link(blender, 184, c1);
    o.link(c0, 48, wrong);
    o.put_f32(c0, 64, 0.5);
    o.link(c1, 48, idle);
    binding(&mut o, c1, "weight", 1);
    binding(&mut o, m, "startStateId", 0);
    states(&mut o, m, 0, &[(0, wrong), (1, blender)]);
    o.file()
}

const RACE: u32 = 0x810;
const KEYWORD: u32 = 0x830;

fn installation(root: &std::path::Path) -> Manifest {
    let files: [(&str, Vec<u8>); 6] = [
        ("meshes/actors/p/project.hkx", project()),
        ("meshes/actors/p/characters/c.hkx", character()),
        ("meshes/actors/p/behaviors/root.hkx", root_graph()),
        ("meshes/actors/p/behaviors/sub.hkx", subgraph()),
        ("meshes/actors/p/animations/mt/idle.hkx", vec![0; 16]),
        ("meshes/actors/p/animations/root.hkx", vec![0; 16]),
    ];
    for (path, bytes) in &files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    let race = [
        sub(b"KWDA", &id(0x831)),
        sub(b"NAM3", &[]),
        sub(b"MNAM", &[]),
        sub(b"MODL", &z("Actors\\P\\Project.hkx")),
        sub(b"FNAM", &[]),
        sub(b"MODL", &z("Actors\\P\\Project.hkx")),
        sub(b"SRAC", &id(0x811)),
    ]
    .concat();
    let entry = |keywords: &[u32], target: &[u32], folders: &[&str], flags: u32| {
        let mut v = Vec::new();
        for k in keywords {
            v.extend(sub(b"SAKD", &id(*k)));
        }
        for k in target {
            v.extend(sub(b"STKD", &id(*k)));
        }
        v.extend(sub(b"SGNM", &z("Actors\\P\\Behaviors\\Sub.hkx")));
        for f in folders {
            v.extend(sub(b"SAPT", &z(f)));
        }
        v.extend(sub(b"SRAF", &id(flags)));
        v
    };
    let subgraph_race = [
        entry(
            &[KEYWORD],
            &[],
            &[
                "Actors\\P\\Animations\\MT\\Other",
                "Actors\\P\\Animations\\MT",
            ],
            0,
        ),
        entry(&[], &[], &["Actors\\P\\Animations"], 0),
        entry(&[], &[KEYWORD], &["Actors\\P\\Animations\\MT"], 0),
        entry(&[], &[], &["Actors\\P\\Animations\\MT"], 1),
    ]
    .concat();
    let body = [
        rec(b"KYWD", KEYWORD, &[]),
        rec(b"KYWD", 0x831, &[]),
        rec(b"RACE", RACE, &race),
        rec(b"RACE", 0x811, &subgraph_race),
        rec(
            b"NPC_",
            0x820,
            &[sub(b"KWDA", &id(KEYWORD)), sub(b"RNAM", &id(RACE))].concat(),
        ),
        rec(b"NPC_", 0x821, &sub(b"RNAM", &id(RACE))),
        rec(
            b"NPC_",
            0x822,
            &[sub(b"TPLT", &id(0x821)), sub(b"RNAM", &id(RACE))].concat(),
        ),
    ]
    .concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let plugin = [rec(b"TES4", 0, &sub(b"HEDR", &hedr)), group_all(&body)].concat();
    std::fs::write(root.join("base.esm"), plugin).unwrap();
    Manifest {
        root: root.to_path_buf(),
        game: Game::Fallout4,
        archive_invalidation: None,
        language: "english".into(),
        plugins: vec![PluginSpec {
            name: "base.esm".into(),
            path: "base.esm".into(),
        }],
        mounts: vec![Mount::Loose {
            plugin: None,
            base: None,
            files: files.iter().map(|(p, _)| (*p).into()).collect(),
        }],
        required_masters: vec![],
        excluded: vec![],
        limits: Limits::default(),
        build: Some("synthetic".into()),
    }
}

/// Top groups of the records, one per type, in order.
fn group_all(records: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    let mut by: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    while at < records.len() {
        let tag: [u8; 4] = records[at..at + 4].try_into().unwrap();
        let len = u32::from_le_bytes(records[at + 4..at + 8].try_into().unwrap()) as usize + 24;
        match by.iter_mut().find(|x| x.0 == tag) {
            Some(x) => x.1.extend(&records[at..at + len]),
            None => by.push((tag, records[at..at + len].to_vec())),
        }
        at += len;
    }
    for (tag, body) in by {
        out.extend(b"GRUP");
        out.extend((body.len() as u32 + 24).to_le_bytes());
        out.extend(tag);
        out.extend(0u32.to_le_bytes());
        out.extend([0; 8]);
        out.extend(body);
    }
    out
}

fn key(id: u32) -> FormKey {
    FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    }
}

const ON: Switches = Switches {
    start: true,
    subgraph: true,
    folder: true,
    cycle: false,
    sit: false,
};

#[test]
fn a_walk_follows_start_states_values_and_weights_to_a_clip_file() {
    let temp = fo4_core::TestDir::new("char-graph");
    let m = installation(&temp.join("Data"));
    let b = Budget::new(64 << 20);
    let store = Store::open(&m, &b).unwrap();
    let vfs = Vfs::mount(&m, &b).unwrap();
    let race = key(RACE);
    let walk = |base: u32, switches: &Switches| {
        let kw = actor_keywords(&store, &key(base), &race, &b).unwrap();
        let input = Actor {
            race: &race,
            section: 0,
            keywords: kw.as_ref().map_err(|s| s.as_str()),
        };
        start_clip(&vfs, &store, &input, switches, &b).unwrap()
    };
    // The keyword actor: entry 0 (one keyword fits), the clip's file name
    // in its second folder.
    let s = walk(0x820, &ON).unwrap();
    assert_eq!(s.clip, "meshes/actors/p/animations/mt/idle.hkx");
    assert_eq!(s.generator.clip, "Animations\\Sub\\Idle.hkt");
    assert_eq!(s.generator.mode, 1);
    assert_eq!(s.subgraph.as_ref().map(|e| e.index), Some(0));
    assert_eq!(s.project, "meshes/actors/p/project.hkx");
    assert_eq!(s.behavior, "meshes/actors/p/behaviors/root.hkx");
    let classes: Vec<&str> = s.steps.iter().map(|x| x.class.as_str()).collect();
    assert_eq!(
        classes,
        [
            hk::STATE_MACHINE,
            hk::MODIFIER_GENERATOR,
            hk::SWAP_GENERATOR,
            hk::STATE_MACHINE,
            hk::BLENDER,
            hk::CLIP_GENERATOR
        ]
    );
    assert_eq!(s.steps[3].how, "start state 1");
    assert_eq!(s.ties, 0);
    assert!(s.rules.contains(hypotheses::CHAR_GRAPH_SUBGRAPH));
    // Without keywords: entry 1, whose folder lacks the file.
    let r = walk(0x821, &ON).unwrap_err();
    assert_eq!(r.code, "GraphClipFile");
    // A templated base: its keywords are not read.
    assert_eq!(walk(0x822, &ON).unwrap_err().code, "GraphActorKeywords");
    // Each switch off stops the walk where it is needed.
    let off = |f: fn(&mut Switches)| {
        let mut s = ON;
        f(&mut s);
        walk(0x820, &s).unwrap_err().code
    };
    assert_eq!(off(|s| s.start = false), "GraphStartSwitchOff");
    assert_eq!(off(|s| s.subgraph = false), "GraphSwapAtRunTime");
    assert_eq!(off(|s| s.folder = false), "GraphClipFolderSwitchOff");
}

#[test]
fn subgraph_entries_ties_and_race_projects() {
    let e = |index: usize, kw: &[u32], target: &[u32], flags: u32| Entry {
        index,
        actor_keywords: kw.iter().map(|k| key(*k)).collect(),
        target_keywords: target.iter().map(|k| key(*k)).collect(),
        behavior: String::new(),
        folders: Vec::new(),
        flags,
    };
    let entries = [
        e(0, &[], &[], 0),
        e(1, &[1], &[], 0),
        e(2, &[2], &[], 0),
        e(3, &[1, 2, 3], &[], 0),
        e(4, &[1, 2], &[], 1),
        e(5, &[1, 2], &[9], 0),
    ];
    let has = |k: &[u32]| k.iter().map(|x| key(*x)).collect::<BTreeSet<_>>();
    assert_eq!(pick_entry(&entries, &has(&[])).unwrap().index, 0);
    assert_eq!(pick_entry(&entries, &has(&[1])).unwrap().index, 1);
    assert!(pick_entry(&entries, &has(&[1, 2]))
        .unwrap_err()
        .contains("2 entries tie"));
    assert_eq!(pick_entry(&entries, &has(&[1, 2, 3])).unwrap().index, 3);
    assert!(pick_entry(&entries[1..3], &has(&[])).is_err());
}

#[test]
fn clip_time_follows_the_mode_only_under_the_loop_switch() {
    let g = |mode: u8, start: f32, speed: f32| hk::ClipGenerator {
        clip: String::new(),
        triggers: None,
        crop_start: 0.,
        crop_end: 0.,
        start_time: start,
        playback_speed: speed,
        enforced_duration: 0.,
        user_fraction: 0.,
        binding_index: -1,
        mode,
        flags: 0,
    };
    assert_eq!(clip_time(&g(1, 0.5, 1.), 4., 10., false), Ok(0.5));
    assert_eq!(clip_time(&g(1, 0.5, 1.), 4., 10., true), Ok(2.5));
    assert_eq!(clip_time(&g(0, 0., 1.), 4., 10., true), Ok(4.));
    assert!(clip_time(&g(2, 0., 1.), 4., 10., true).is_err());
    assert!(clip_time(&g(1, 0., 2.), 4., 10., true).is_err());
    assert!(clip_time(&g(1, 0., 1.), 0., 10., true).is_err());
}

/// A furniture's entry: flag word 2, no actor keywords, every target
/// keyword the furniture's, the most of them; a tie or none refused
/// (`char_graph_furniture_entry_keywords_v1`).
#[test]
fn a_furniture_entry_is_the_word_2_entry_with_the_most_of_its_keywords() {
    let kw = |id: u32| FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    };
    let entry = |index: usize, flags: u32, actor: &[u32], target: &[u32]| Entry {
        index,
        actor_keywords: actor.iter().map(|i| kw(*i)).collect(),
        target_keywords: target.iter().map(|i| kw(*i)).collect(),
        behavior: format!("b{index}.hkx"),
        folders: Vec::new(),
        flags,
    };
    let entries = [
        entry(0, 0, &[], &[1]),
        entry(1, 2, &[], &[1]),
        entry(2, 2, &[], &[1, 2]),
        entry(3, 2, &[9], &[1, 2, 3]),
        entry(4, 2, &[], &[4]),
        entry(5, 2, &[], &[5]),
        entry(6, 2, &[], &[5]),
    ];
    let has = |ids: &[u32]| ids.iter().map(|i| kw(*i)).collect::<BTreeSet<FormKey>>();
    assert_eq!(
        pick_furniture_entry(&entries, &has(&[1, 2, 3]))
            .unwrap()
            .index,
        2
    );
    assert_eq!(pick_furniture_entry(&entries, &has(&[1])).unwrap().index, 1);
    assert!(pick_furniture_entry(&entries, &has(&[5]))
        .unwrap_err()
        .contains("tie"));
    assert!(pick_furniture_entry(&entries, &has(&[7])).is_err());
}
