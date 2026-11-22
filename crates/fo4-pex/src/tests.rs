//! Synthetic files built by hand; no game data.
use super::*;

/// Little writer for hand-built files.
#[derive(Default)]
struct W(Vec<u8>);
impl W {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn text(&mut self, t: &str) -> &mut Self {
        self.u16(t.len() as u16);
        self.0.extend(t.as_bytes());
        self
    }
    fn ident(&mut self, i: u16) -> &mut Self {
        self.u8(1).u16(i)
    }
    fn int(&mut self, v: i32) -> &mut Self {
        self.u8(3).u32(v as u32)
    }
}

const STRINGS: [&str; 16] = [
    "Demo",      // 0 object
    "",          // 1
    "Base",      // 2 parent
    "Int",       // 3
    "count",     // 4 variable
    "Count",     // 5 property
    "OnTick",    // 6 function
    "None",      // 7
    "::nonevar", // 8
    "self",      // 9
    "Step",      // 10
    "hidden",    // 11 user flag
    "Pair",      // 12 struct
    "a",         // 13 member
    "Float",     // 14
    "busy",      // 15 state
];

/// A file with debug info, one user flag, one object holding a struct, a
/// variable, a variable-backed property, and two states; the empty state's
/// function holds a counted call, an op with three slots and a jump.
fn fixture() -> Vec<u8> {
    let mut w = W::default();
    w.u32(MAGIC).u8(3).u8(9).u16(GAME).u64(1_000);
    w.text("x.psc").text("u").text("m");
    w.u16(STRINGS.len() as u16);
    for s in STRINGS {
        w.text(s);
    }
    // Debug: one function entry, one property group, one struct order.
    w.u8(1).u64(900);
    w.u16(1)
        .u16(0)
        .u16(1)
        .u16(6)
        .u8(0)
        .u16(3)
        .u16(10)
        .u16(11)
        .u16(12);
    w.u16(1).u16(0).u16(1).u16(1).u32(0).u16(1).u16(5);
    w.u16(1).u16(0).u16(12).u16(1).u16(13);
    // User flags.
    w.u16(1).u16(11).u8(0);
    // Objects.
    w.u16(1).u16(0);
    let size_at = w.0.len();
    w.u32(0).u16(2).u16(1).u8(0).u32(1).u16(1);
    // Struct Pair { Float a = 1.5 }.
    w.u16(1).u16(12).u16(1);
    w.u16(13)
        .u16(14)
        .u32(0)
        .u8(4)
        .u32(1.5f32.to_bits())
        .u8(0)
        .u16(1);
    // Variable Int count = 3.
    w.u16(1).u16(4).u16(3).u32(0).int(3).u8(0);
    // Property Int Count backed by count.
    w.u16(1).u16(5).u16(3).u16(1).u32(1).u8(7).u16(4);
    // States: "" with OnTick, "busy" with OnTick.
    w.u16(2);
    w.u16(1).u16(1).u16(6);
    w.u16(7).u16(1).u32(0).u8(0).u16(0).u16(1).u16(8).u16(7);
    w.u16(3);
    // CallMethod Step, self, ::nonevar, 1 (count)
    w.u8(23).ident(10).ident(9).ident(8).int(1).ident(4);
    // Op01 count, count, 1
    w.u8(1).ident(4).ident(4).int(1);
    // Jump -2
    w.u8(20).int(-2);
    w.u16(15).u16(1).u16(6);
    w.u16(7).u16(1).u32(0).u8(0).u16(0).u16(0).u16(0);
    let size = (w.0.len() - size_at) as u32;
    w.0[size_at..size_at + 4].copy_from_slice(&size.to_le_bytes());
    w.0
}

fn read(d: &[u8]) -> Result<Pex<'_>> {
    Pex::read(d, "fixture.pex", &Budget::new(1 << 20))
}

#[test]
fn reads_every_part_of_a_hand_built_file() {
    let d = fixture();
    let p = read(&d).unwrap();
    assert_eq!(p.header.compiled, 1_000);
    assert_eq!(p.header.source, b"x.psc");
    assert_eq!(p.strings.len(), 16);
    assert_eq!(p.sections.end as usize, d.len());
    let dbg = p.debug.as_ref().unwrap();
    assert_eq!(dbg.modified, 900);
    assert_eq!(dbg.functions[0].lines, [10, 11, 12]);
    assert_eq!(dbg.property_groups[0].properties, [5]);
    assert_eq!(dbg.struct_orders[0].members, [13]);
    assert_eq!(p.user_flags[0].bit, 0);
    let o = &p.objects[0];
    assert_eq!((o.name, o.parent, o.user_flags), (0, 2, 1));
    assert_eq!(o.structs[0].members[0].value, Value::Float(1.5));
    assert_eq!(o.variables[0].value, Value::Int(3));
    assert_eq!(o.properties[0].variable, Some(4));
    assert!(o.properties[0].read.is_none());
    let code = &o.states[0].functions[0].function.code;
    assert_eq!(code.len(), 3);
    assert_eq!(code[0].op, 23);
    assert_eq!(code[0].args.len(), 4);
    assert_eq!(code[0].extra, [Value::Ident(4)]);
    assert_eq!(code[2].args, [Value::Int(-2)]);
    let total: usize = code.iter().map(Instruction::stored_len).sum();
    assert_eq!(total, 18 + 12 + 6);
    assert_eq!(p.functions().len(), 2);
}

#[test]
fn disassembly_names_claimed_roles_and_keeps_others_numbered() {
    let d = fixture();
    let p = read(&d).unwrap();
    let t = disassemble(&p);
    assert!(t.contains("object Demo extends \"Base\" [hidden]"), "{t}");
    assert!(
        t.contains("CallMethod     Step, self, ::nonevar, 1 (count)    ; line 10"),
        "{t}"
    );
    assert!(
        t.contains("Op01           count, count, 1    ; line 11"),
        "{t}"
    );
    assert!(
        t.contains("Jump           -2 (-> 0000)    ; line 12"),
        "{t}"
    );
    assert!(t.contains("state \"busy\""), "{t}");
    assert!(
        t.contains("property Int Count flags=7 variable count"),
        "{t}"
    );
}

#[test]
fn every_truncation_is_a_named_error() {
    let d = fixture();
    for n in 0..d.len() {
        let e = read(&d[..n]).unwrap_err();
        assert!(
            ["Truncated", "ObjectSize"].contains(&e.code.as_str()),
            "{n}: {e}"
        );
    }
}

#[test]
fn trailing_bytes_and_wrong_sizes_are_refused() {
    let mut d = fixture();
    d.push(0);
    assert_eq!(read(&d).unwrap_err().code, "TrailingBytes");
    let d = fixture();
    let at = d.len() - read(&d).unwrap().objects[0].size as usize;
    let mut bad = d.clone();
    bad[at] ^= 1;
    assert_eq!(read(&bad).unwrap_err().code, "ObjectSize");
    let mut bad = d.clone();
    bad[0] ^= 1;
    assert_eq!(read(&bad).unwrap_err().code, "Magic");
    let mut bad = d;
    bad[5] = 10;
    assert_eq!(read(&bad).unwrap_err().code, "Version");
}

#[test]
fn unmeasured_opcodes_kinds_and_indices_are_refused() {
    let d = fixture();
    let p = read(&d).unwrap();
    let code = &p.objects[0].states[0].functions[0].function.code;
    let op_at = code[1].offset as usize;
    for op in [0u8, 35, 41, 44, 47, 255] {
        let mut bad = d.clone();
        bad[op_at] = op;
        assert_eq!(read(&bad).unwrap_err().code, "Opcode", "{op}");
    }
    let mut bad = d.clone();
    bad[op_at + 1] = 6;
    assert_eq!(read(&bad).unwrap_err().code, "ValueKind");
    let mut bad = d.clone();
    bad[op_at + 2] = 16;
    assert_eq!(read(&bad).unwrap_err().code, "StringIndex");
    // A negative count in a counted list.
    let call_count = code[0].offset as usize + 1 + 9;
    let mut bad = d;
    bad[call_count + 1..call_count + 5].copy_from_slice(&(-1i32).to_le_bytes());
    assert_eq!(read(&bad).unwrap_err().code, "ArgumentCount");
}

#[test]
fn a_changed_arity_misreads_the_fixture() {
    let d = fixture();
    let b = Budget::new(1 << 20);
    for (op, shape) in [
        (
            1usize,
            Shape {
                fixed: 2,
                counted: false,
            },
        ),
        (
            20,
            Shape {
                fixed: 2,
                counted: false,
            },
        ),
        (
            23,
            Shape {
                fixed: 4,
                counted: false,
            },
        ),
    ] {
        let mut t = OPCODES;
        t[op] = Some(shape);
        assert!(Pex::read_with(&d, "fixture.pex", &b, &t).is_err(), "{op}");
    }
}

#[test]
fn the_budget_bounds_what_is_kept() {
    let d = fixture();
    let e = Pex::read(&d, "fixture.pex", &Budget::new(64)).unwrap_err();
    assert_eq!(e.code, "LimitExceeded");
}

#[test]
fn tables_cover_the_same_opcodes() {
    assert_eq!(OPCODES.len(), ROLES.len());
    for (op, r) in ROLES.iter().enumerate() {
        if *r != Role::Unclaimed {
            assert!(OPCODES[op].is_some(), "{op}");
        }
    }
    assert_eq!(op_name(7), "Op07");
    assert_eq!(op_name(25), "CallStatic");
}
