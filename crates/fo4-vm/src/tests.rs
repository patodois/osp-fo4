//! Hand-built synthetic scripts only: a small text assembler builds
//! `fo4_pex` structures, which [`Script::from_pex`] copies. No game data.
use super::*;

mod cond2;
mod data;
mod events;
mod open2;
mod quest;
mod scene;
mod scev;
mod stage;
mod talk;
mod world;

use fo4_pex::{
    Function, Header, Instruction, Member, NamedFunction, Object, Pex, Property, Sections, State,
    Str, Struct, Value, Variable, FUNCTION_GLOBAL, FUNCTION_NATIVE, OPCODES, PROPERTY_READ,
    PROPERTY_VARIABLE, PROPERTY_WRITE,
};

/// Assembles one script from text:
///
/// ```text
/// script NAME [extends PARENT] [auto STATE]
/// var TYPE NAME [LITERAL]
/// prop TYPE NAME VARIABLE
/// struct NAME
/// member TYPE NAME [LITERAL]
/// state NAME            (functions below go to it; "-" is the empty state)
/// func NAME RET [native] [global] [TYPE PARAM]...
/// local TYPE NAME
/// OP ARG...             (all fixed slots, the count included, then the counted ones)
/// ```
///
/// Arguments: integers, floats (with a dot), `true`, `false`, `none`,
/// `"text"`, anything else a name.
fn assemble(src: &str) -> Script {
    let mut strings: Vec<Vec<u8>> = Vec::new();
    let mut s = |t: &str| -> Str {
        if let Some(i) = strings.iter().position(|x| x == t.as_bytes()) {
            return i as Str;
        }
        strings.push(t.as_bytes().to_vec());
        (strings.len() - 1) as Str
    };
    let empty = s("");
    let lit = |t: &str, s: &mut dyn FnMut(&str) -> Str| -> Value {
        if let Ok(n) = t.parse::<i32>() {
            Value::Int(n)
        } else if t.contains('.') && t.parse::<f32>().is_ok() {
            Value::Float(t.parse().unwrap())
        } else if t == "true" || t == "false" {
            Value::Bool(t == "true")
        } else if t == "none" {
            Value::None
        } else if let Some(x) = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
            Value::Str(s(x))
        } else {
            Value::Ident(s(t))
        }
    };
    let mut o = Object {
        name: empty,
        offset: 0,
        size: 0,
        parent: empty,
        doc: empty,
        constant: 0,
        user_flags: 0,
        auto_state: empty,
        structs: Vec::new(),
        variables: Vec::new(),
        properties: Vec::new(),
        states: vec![State {
            name: empty,
            functions: Vec::new(),
        }],
    };
    let mut state = 0usize;
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let w: Vec<&str> = tokens(line);
        match w[0] {
            "script" => {
                o.name = s(w[1]);
                let mut i = 2;
                while i + 1 < w.len() {
                    match w[i] {
                        "extends" => o.parent = s(w[i + 1]),
                        "auto" => o.auto_state = s(w[i + 1]),
                        x => panic!("script option {x}"),
                    }
                    i += 2;
                }
            }
            "var" => o.variables.push(Variable {
                type_name: s(w[1]),
                name: s(w[2]),
                user_flags: 0,
                value: w.get(3).map_or(Value::None, |t| lit(t, &mut s)),
                constant: 0,
            }),
            "prop" => o.properties.push(Property {
                type_name: s(w[1]),
                name: s(w[2]),
                doc: empty,
                user_flags: 0,
                flags: PROPERTY_READ | PROPERTY_WRITE | PROPERTY_VARIABLE,
                variable: Some(s(w[3])),
                read: None,
                write: None,
            }),
            "struct" => o.structs.push(Struct {
                name: s(w[1]),
                members: Vec::new(),
            }),
            "member" => {
                let m = Member {
                    type_name: s(w[1]),
                    name: s(w[2]),
                    user_flags: 0,
                    value: w.get(3).map_or(Value::None, |t| lit(t, &mut s)),
                    constant: 0,
                    doc: empty,
                };
                o.structs.last_mut().unwrap().members.push(m);
            }
            "state" => {
                let n = if w[1] == "-" { empty } else { s(w[1]) };
                state = match o.states.iter().position(|x| x.name == n) {
                    Some(i) => i,
                    None => {
                        o.states.push(State {
                            name: n,
                            functions: Vec::new(),
                        });
                        o.states.len() - 1
                    }
                };
            }
            "func" => {
                let mut flags = 0;
                let mut params = Vec::new();
                let mut i = 3;
                while i < w.len() {
                    match w[i] {
                        "native" => flags |= FUNCTION_NATIVE,
                        "global" => flags |= FUNCTION_GLOBAL,
                        t => {
                            params.push((s(t), s(w[i + 1])));
                            i += 1;
                        }
                    }
                    i += 1;
                }
                let params = params.into_iter().map(|(t, n)| (n, t)).collect();
                let f = NamedFunction {
                    name: s(w[1]),
                    function: Function {
                        return_type: s(w[2]),
                        doc: empty,
                        user_flags: 0,
                        flags,
                        params,
                        locals: Vec::new(),
                        code: Vec::new(),
                    },
                };
                o.states[state].functions.push(f);
            }
            "local" => {
                let l = (s(w[2]), s(w[1]));
                let f = o.states[state].functions.last_mut().unwrap();
                f.function.locals.push(l);
            }
            op => {
                let op: u8 = op.parse().unwrap_or_else(|_| panic!("line {line}"));
                let vals: Vec<Value> = w[1..].iter().map(|t| lit(t, &mut s)).collect();
                let shape = OPCODES.get(usize::from(op)).copied().flatten();
                let fixed = shape.map_or(vals.len(), |x| usize::from(x.fixed));
                let f = o.states[state].functions.last_mut().unwrap();
                f.function.code.push(Instruction {
                    offset: 0,
                    op,
                    args: vals[..fixed].to_vec(),
                    extra: vals[fixed..].to_vec(),
                });
            }
        }
    }
    let p = Pex {
        header: Header {
            major: 3,
            minor: 9,
            game: 2,
            compiled: 0,
            source: b"",
            user: b"",
            machine: b"",
        },
        sections: Sections {
            strings: 0,
            debug: 0,
            user_flags: 0,
            objects: 0,
            end: 0,
        },
        strings: strings.iter().map(Vec::as_slice).collect(),
        debug: None,
        user_flags: Vec::new(),
        objects: vec![o],
    };
    Script::from_pex(&p, 0).unwrap()
}

fn tokens(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find(|c: char| !c.is_whitespace()) {
        rest = &rest[start..];
        let end = if let Some(quoted) = rest.strip_prefix('"') {
            quoted.find('"').map_or(rest.len(), |e| e + 2)
        } else {
            rest.find(char::is_whitespace).unwrap_or(rest.len())
        };
        out.push(&rest[..end]);
        rest = &rest[end..];
    }
    out
}

/// A base library shaped like the measured one: a root script whose state
/// functions use `::State`, a reference type with natives, a global native.
fn base() -> MapLibrary {
    let mut l = MapLibrary::default();
    l.add(assemble(
        "script ScriptObject
         func GotoState none String asNewState
         local none ::nonevar
         local String soldState
         13 soldState ::State
         23 OnEndState self ::nonevar 1 asNewState
         13 ::State asNewState
         23 OnBeginState self ::nonevar 1 soldState
         func GetState String
         26 ::State
         func OnBeginState none String asOldState
         func OnEndState none String asNewState
         func OnTimer none Int aiTimerID
         func OnInit none
         func StartTimer none native Float afInterval Int aiTimerID
         func StartTimerGameTime none native Float afInterval Int aiTimerID
         func CancelTimer none native Int aiTimerID",
    ));
    l.add(assemble("script Form extends ScriptObject"));
    l.add(assemble(
        "script ObjectReference extends Form
         func OnActivate none ObjectReference akActionRef
         func OnLoad none
         func Disable none native Bool abFade
         func Enable none native Bool abFade
         func Delete none native
         func IsEnabled Bool native
         func AddItem none native Form akItem Int aiCount Bool abSilent
         func GetLinkedRef ObjectReference native Keyword kw",
    ));
    l.add(assemble("script Actor extends ObjectReference"));
    l.add(assemble("script Keyword extends Form"));
    l.add(assemble(
        "script Game
         func GetPlayer Actor native global",
    ));
    l
}

fn new_ref(vm: &mut Vm<'_>, script: &str) -> InstId {
    let o = vm
        .world
        .object("ref", Some("ref:1".into()), &["objectreference"]);
    vm.instantiate(Some(o), script).unwrap()
}

#[test]
fn arithmetic_loop_runs_and_the_swapped_jump_sense_does_not_terminate() {
    let mut lib = base();
    lib.add(assemble(
        "script MathUtil
         func SumTo Int global Int n
         local Int i
         local Int total
         local Bool t
         16 t i n
         22 t 4
         1 i i 1
         1 total total i
         20 -4
         26 total",
    ));
    let f = lib.scripts["mathutil"]
        .function("", "sumto")
        .unwrap()
        .clone();
    let mut host = NoHost;
    let mut vm = Vm::new(&mut lib, &mut host);
    assert_eq!(
        vm.run(None, f.clone(), vec![Val::Int(10)]),
        Outcome::Completed(Val::Int(55))
    );
    assert!(vm.rules.contains(rules::ARITHMETIC) && vm.rules.contains(rules::JUMPS));
    vm.variant = Variant::JumpSenseSwapped;
    vm.limits.steps = 10_000;
    assert_eq!(
        vm.run(None, f.clone(), vec![Val::Int(10)]),
        Outcome::Completed(Val::Int(0))
    );
    vm.variant = Variant::LessGreaterSwapped;
    assert_eq!(
        vm.run(None, f.clone(), vec![Val::Int(10)]),
        Outcome::Completed(Val::Int(0))
    );
    vm.variant = Variant::StrictSwapped;
    assert_eq!(
        vm.run(None, f, vec![Val::Int(10)]),
        Outcome::Completed(Val::Int(66))
    );
}

#[test]
fn a_native_result_that_steers_control_flow_blocks_the_run() {
    let mut lib = base();
    lib.add(assemble(
        "script Lamp extends ObjectReference
         func OnLoad none
         local Bool b
         local none ::nonevar
         23 IsEnabled self b 0
         22 b 2
         23 Disable self ::nonevar 1 false",
    ));
    let mut host = RecordingHost::default();
    let mut vm = Vm::new(&mut lib, &mut host);
    let i = new_ref(&mut vm, "lamp");
    let o = vm.deliver(i, "onload", vec![]);
    let Outcome::Blocked(b) = o else {
        panic!("{o:?}")
    };
    assert_eq!(b.on, Origin::Native("objectreference.isenabled".into()));
    assert_eq!(b.at.unwrap().index, 1);
    assert_eq!(vm.effects.len(), 1);
    assert_eq!(vm.effects[0].native, "objectreference.isenabled");
    assert!(vm.effects[0].result.starts_with("?0"));
}

const PICKUP: &str = "script Pickup extends ObjectReference
     var Form ::Item_var
     prop Form Item ::Item_var
     func OnActivate none ObjectReference akActionRef
     local none ::nonevar
     local Form ::temp0
     14 ::temp0 ::Item_var
     23 AddItem akActionRef ::nonevar 3 ::temp0 1 false
     23 Disable self ::nonevar 1 false
     23 Delete self ::nonevar 0";

#[test]
fn an_activation_runs_to_completion_and_logs_every_native() {
    let mut lib = base();
    lib.add(assemble(PICKUP));
    let mut host = RecordingHost::default();
    let mut vm = Vm::new(&mut lib, &mut host);
    let i = new_ref(&mut vm, "pickup");
    let item = vm.world.keyed_object("item", "form:2", "form");
    assert!(vm
        .set_property_variable(i, "item", Val::Object(item))
        .unwrap());
    let player = vm.host_player();
    let o = vm.deliver(i, "onactivate", vec![player]);
    assert_eq!(o, Outcome::Completed(Val::None));
    let log: Vec<_> = vm
        .effects
        .iter()
        .map(|e| {
            format!(
                "{} {} ({})",
                e.native,
                e.receiver.clone().unwrap_or_default(),
                e.args.join(", ")
            )
        })
        .collect();
    assert_eq!(
        log,
        [
            "objectreference.additem <player> (<item>, 1, false)",
            "objectreference.disable <ref> (false)",
            "objectreference.delete <ref> ()",
        ]
    );
    let me = vm.world.instances[i].object;
    assert_eq!(vm.effects[1].receiver_object, me);
}

impl Vm<'_> {
    /// The recording host's player handle (tests only).
    fn host_player(&mut self) -> Val {
        Val::Object(self.world.player("actor"))
    }
}

#[test]
fn the_player_handle_equals_itself_and_casts_up_but_not_down() {
    let mut lib = base();
    lib.add(assemble(
        "script Trig extends ObjectReference
         func OnTriggerEnter none ObjectReference akActionRef
         local Actor ::temp1
         local ObjectReference ::temp2
         local Bool ::temp3
         local none ::nonevar
         25 Game GetPlayer ::temp1 0
         14 ::temp2 ::temp1
         15 ::temp3 akActionRef ::temp2
         22 ::temp3 2
         23 Disable self ::nonevar 1 false
         func Down none ObjectReference akRef
         local Actor a
         local none ::nonevar
         14 a akRef
         22 a 2
         23 Disable self ::nonevar 1 false",
    ));
    let mut host = RecordingHost::default();
    let mut vm = Vm::new(&mut lib, &mut host);
    let i = new_ref(&mut vm, "trig");
    let p = vm.host.call(
        &NativeCall {
            script: "game".into(),
            name: "getplayer".into(),
            receiver: None,
            instance: None,
            args: vec![],
            param_types: vec![],
            return_type: "actor".into(),
        },
        &mut vm.world,
    );
    let Reply::Value(player) = p else { panic!() };
    assert_eq!(
        vm.deliver(i, "ontriggerenter", vec![player.clone()]),
        Outcome::Completed(Val::None)
    );
    assert_eq!(vm.effects.last().unwrap().native, "objectreference.disable");
    assert!(vm.rules.contains(rules::HOST_PLAYER));
    // Another reference is not known to be an Actor: the cast is open.
    let other = vm
        .world
        .object("other", Some("ref:9".into()), &["objectreference"]);
    let o = vm.deliver(i, "down", vec![Val::Object(other)]);
    let Outcome::Blocked(b) = o else {
        panic!("{o:?}")
    };
    assert!(matches!(b.on, Origin::Rule(rules::NOT_COPY_CAST, _)));
    // The player casts down to its known type.
    let n = vm.effects.len();
    assert_eq!(
        vm.deliver(i, "down", vec![player]),
        Outcome::Completed(Val::None)
    );
    assert_eq!(vm.effects.len(), n + 1);
}

#[test]
fn goto_state_runs_through_the_state_identifier_and_changes_dispatch() {
    let mut lib = base();
    lib.add(assemble(
        "script Trap extends ObjectReference auto Armed
         var Int hits 0
         func OnActivate none ObjectReference akActionRef
         state Armed
         func OnActivate none ObjectReference akActionRef
         local none ::nonevar
         23 GotoState self ::nonevar 1 \"Fired\"
         state Fired
         func OnBeginState none String asOldState
         1 hits hits 1
         func OnActivate none ObjectReference akActionRef
         3 hits hits 100",
    ));
    let mut host = RecordingHost::default();
    let mut vm = Vm::new(&mut lib, &mut host);
    let i = new_ref(&mut vm, "trap");
    assert_eq!(vm.world.instances[i].state, "armed");
    assert_eq!(
        vm.deliver(i, "onactivate", vec![Val::None]),
        Outcome::Completed(Val::None)
    );
    assert_eq!(vm.world.instances[i].state, "fired");
    assert_eq!(vm.world.instances[i].vars[0].value, Val::Int(1));
    vm.deliver(i, "onactivate", vec![Val::None]);
    assert_eq!(vm.world.instances[i].vars[0].value, Val::Int(-99));
    assert!(vm.rules.contains(rules::STATE));
}

#[test]
fn an_opcode_without_a_candidate_meaning_stops_the_run() {
    let mut lib = base();
    let mut s = assemble(
        "script Odd
         func F none global
         26 none",
    );
    let f = s.functions.values_mut().next().unwrap();
    Rc::get_mut(f).unwrap().code[0].op = 35;
    let f = f.clone();
    lib.add(s);
    let mut host = NoHost;
    let mut vm = Vm::new(&mut lib, &mut host);
    let o = vm.run(None, f, vec![]);
    assert!(matches!(
        o,
        Outcome::Failed(VmError::UnmeasuredOpcode { site }) if site.op == 35
    ));
}

#[test]
fn a_native_the_host_does_not_provide_stops_the_run() {
    let mut lib = base();
    lib.add(assemble(PICKUP));
    let mut host = NoHost;
    let mut vm = Vm::new(&mut lib, &mut host);
    let i = new_ref(&mut vm, "pickup");
    let p = vm.host_player();
    let o = vm.deliver(i, "onactivate", vec![p]);
    assert_eq!(
        o,
        Outcome::Failed(VmError::NativeNotProvided("objectreference.additem".into()))
    );
}

#[test]
fn step_and_depth_limits_stop_endless_runs() {
    let mut lib = base();
    lib.add(assemble(
        "script Loops
         func Spin none global
         20 0
         func Deep none global
         local none ::nonevar
         25 Loops Deep ::nonevar 0",
    ));
    let get = |l: &MapLibrary, n: &str| l.scripts["loops"].function("", n).unwrap().clone();
    let (spin, deep) = (get(&lib, "spin"), get(&lib, "deep"));
    let mut host = NoHost;
    let mut vm = Vm::new(&mut lib, &mut host);
    // A jump by 0 is refused (no measured jump has it).
    assert!(matches!(
        vm.run(None, spin, vec![]),
        Outcome::Failed(VmError::BadArgument { .. })
    ));
    let mut lib2 = base();
    lib2.add(assemble(
        "script Loops2
         func Forever none global
         local Bool t
         13 t true
         21 t -1",
    ));
    let forever = lib2.scripts["loops2"]
        .function("", "forever")
        .unwrap()
        .clone();
    assert_eq!(
        vm.run(None, deep, vec![]),
        Outcome::Failed(VmError::DepthLimit(64))
    );
    let mut host2 = NoHost;
    let mut vm2 = Vm::new(&mut lib2, &mut host2);
    vm2.limits.steps = 1000;
    assert_eq!(
        vm2.run(None, forever, vec![]),
        Outcome::Failed(VmError::StepLimit(1000))
    );
}

#[test]
fn array_operations_follow_their_candidate_meanings() {
    let mut lib = base();
    lib.add(assemble(
        "script Arr
         func F Int[] global
         local Int[] a
         local Int n
         local Int x
         30 a 3
         33 a 0 5
         33 a 2 7
         42 a 9 2
         43 a 4 1
         45 a 0 1
         31 n a
         32 x a 1
         34 a n 9 0
         26 a
         func Oob Int global
         local Int[] a
         local Int x
         30 a 2
         32 x a 2
         26 x",
    ));
    let f = lib.scripts["arr"].function("", "f").unwrap().clone();
    let oob = lib.scripts["arr"].function("", "oob").unwrap().clone();
    let mut host = NoHost;
    let mut vm = Vm::new(&mut lib, &mut host);
    let Outcome::Completed(Val::Array(a)) = vm.run(None, f, vec![]) else {
        panic!()
    };
    // [5,0,7] + [9,9] -> insert 4 at 1 -> [5,4,0,7,9,9] -> remove 1 at 0
    assert_eq!(
        vm.world.arrays[a].items,
        [4, 0, 7, 9, 9].map(Val::Int).to_vec()
    );
    assert!(vm.rules.contains(rules::DEFAULTS));
    assert!(matches!(
        vm.run(None, oob, vec![]),
        Outcome::Failed(VmError::IndexOutOfRange {
            index: 2,
            len: 2,
            ..
        })
    ));
}

#[test]
fn struct_operations_and_struct_find() {
    let mut lib = base();
    lib.add(assemble(
        "script Kit
         struct Pair
         member Int a 3
         member Float b
         func F Int global
         local Kit#Pair p
         local Kit#Pair q
         local Kit#Pair[] all
         local Int x
         37 p
         39 p b 2.5
         37 q
         39 q a 8
         30 all 2
         33 all 0 p
         33 all 1 q
         40 all x \"a\" 8 0
         38 x q a
         40 all x \"a\" 8 0
         26 x",
    ));
    let f = lib.scripts["kit"].function("", "f").unwrap().clone();
    let mut host = NoHost;
    let mut vm = Vm::new(&mut lib, &mut host);
    assert_eq!(vm.run(None, f, vec![]), Outcome::Completed(Val::Int(1)));
    assert_eq!(vm.world.structs[0].members[1].2, Val::Float(2.5));
    assert!(vm.rules.contains(rules::STRUCTS) && vm.rules.contains(rules::ARRAYS));
}

#[test]
fn timers_queue_and_fire_in_due_order() {
    let mut lib = base();
    lib.add(assemble(
        "script Clocked extends ObjectReference
         var Int last 0
         func OnLoad none
         local none ::nonevar
         23 StartTimer self ::nonevar 2 5.0 2
         23 StartTimer self ::nonevar 2 1.0 1
         func OnTimer none Int aiTimerID
         13 last aiTimerID",
    ));
    let mut host = RecordingHost::default();
    let mut vm = Vm::new(&mut lib, &mut host);
    let i = new_ref(&mut vm, "clocked");
    assert_eq!(
        vm.deliver(i, "onload", vec![]),
        Outcome::Completed(Val::None)
    );
    let fired = vm.fire_timers(10);
    let ids: Vec<_> = fired.iter().map(|x| x.1.clone()).collect();
    assert_eq!(ids, [Val::Int(1), Val::Int(2)]);
    assert_eq!(vm.world.instances[i].vars[0].value, Val::Int(2));
    assert!(vm.rules.contains(rules::TIMERS));
}

#[test]
fn the_time_scale_orders_game_time_timers_against_real_time_ones() {
    let mut lib = base();
    lib.add(assemble(
        "script Clocked2 extends ObjectReference
         var Int last 0
         func OnLoad none
         local none ::nonevar
         23 StartTimerGameTime self ::nonevar 2 1.0 7
         23 StartTimer self ::nonevar 2 100.0 8
         func OnTimer none Int aiTimerID
         13 last aiTimerID
         func OnTimerGameTime none Int aiTimerID
         13 last aiTimerID",
    ));
    let mut host = RecordingHost::default();
    let mut vm = Vm::new(&mut lib, &mut host);
    let i = new_ref(&mut vm, "clocked2");
    vm.deliver(i, "onload", vec![]);
    // Both clocks queued: without a scale the firing stops.
    assert_eq!(vm.next_timer_wait(None), None);
    let x = vm.fire_timer_scaled(None).unwrap();
    assert!(matches!(x.2, Outcome::Failed(VmError::Timers(_))));
    // One game hour at scale 20 is 180 real seconds: the real-time timer
    // (100 s) fires first, then the game-time one; the wait a caller can
    // ask beforehand is the one the firing takes.
    assert_eq!(vm.next_timer_wait(Some(20.0)), Some(100.0));
    assert_eq!(vm.fire_timer_scaled(Some(20.0)).unwrap().1, Val::Int(8));
    assert_eq!(vm.next_timer_wait(Some(20.0)), Some(80.0));
    assert_eq!(vm.fire_timer_scaled(Some(20.0)).unwrap().1, Val::Int(7));
    assert_eq!(vm.next_timer_wait(Some(20.0)), None);
    assert!(vm.rules.contains(rules::TIME_SCALE));
    // At scale 0 the game clock stands still: nothing is due.
    vm.deliver(i, "onload", vec![]);
    assert_eq!(vm.fire_timer_scaled(Some(0.0)).unwrap().1, Val::Int(8));
    assert!(vm.fire_timer_scaled(Some(0.0)).is_none());
}

#[test]
fn values_no_rule_settles_stay_open() {
    let mut lib = base();
    lib.add(assemble(
        "script Open
         func Div Int global Int a Int b
         local Int r
         7 r a b
         26 r
         func Same Bool global String a String b
         local Bool r
         15 r a b
         26 r
         func Text String global Float x
         local String r
         14 r x
         26 r",
    ));
    let g = |n: &str| lib.scripts["open"].function("", n).unwrap().clone();
    let (div, same, text) = (g("div"), g("same"), g("text"));
    let mut host = NoHost;
    let mut vm = Vm::new(&mut lib, &mut host);
    assert_eq!(
        vm.run(None, div.clone(), vec![Val::Int(7), Val::Int(2)]),
        Outcome::Completed(Val::Int(3))
    );
    assert!(matches!(
        vm.run(None, div.clone(), vec![Val::Int(-7), Val::Int(2)]),
        Outcome::Completed(Val::Unknown(_))
    ));
    assert!(matches!(
        vm.run(None, div, vec![Val::Int(1), Val::Int(0)]),
        Outcome::Failed(VmError::DivisionByZero { .. })
    ));
    let s = |t: &str| Val::Str(t.into());
    assert_eq!(
        vm.run(None, same.clone(), vec![s("ab"), s("ab")]),
        Outcome::Completed(Val::Bool(true))
    );
    assert_eq!(
        vm.run(None, same.clone(), vec![s("ab"), s("cd")]),
        Outcome::Completed(Val::Bool(false))
    );
    assert!(matches!(
        vm.run(None, same, vec![s("ab"), s("AB")]),
        Outcome::Completed(Val::Unknown(_))
    ));
    assert!(matches!(
        vm.run(None, text, vec![Val::Float(1.5)]),
        Outcome::Completed(Val::Unknown(_))
    ));
}

#[test]
fn a_scripted_call_on_an_object_without_that_script_blocks() {
    let mut lib = base();
    lib.add(assemble(
        "script QuestLike extends Form
         var Int stage 0
         func SetStage none Int n
         13 stage n",
    ));
    lib.add(assemble(
        "script Caller extends ObjectReference
         var QuestLike ::Q_var
         prop QuestLike Q ::Q_var
         func OnActivate none ObjectReference akActionRef
         local none ::nonevar
         23 SetStage ::Q_var ::nonevar 1 5",
    ));
    let mut host = RecordingHost::default();
    let mut vm = Vm::new(&mut lib, &mut host);
    let i = new_ref(&mut vm, "caller");
    let q = vm.world.keyed_object("quest", "form:3", "questlike");
    vm.set_property_variable(i, "q", Val::Object(q)).unwrap();
    let o = vm.deliver(i, "onactivate", vec![Val::None]);
    assert_eq!(
        o,
        Outcome::Blocked(Block {
            on: Origin::NoInstance("questlike".into(), "record ques".into()),
            at: Some(Site {
                script: "caller".into(),
                state: String::new(),
                function: "onactivate".into(),
                index: 0,
                op: 23
            })
        })
    );
    // With an instance attached, the call runs on it.
    let qi = vm.instantiate(Some(q), "questlike").unwrap();
    assert_eq!(
        vm.deliver(i, "onactivate", vec![Val::None]),
        Outcome::Completed(Val::None)
    );
    assert_eq!(vm.world.instances[qi].vars[0].value, Val::Int(5));
}

#[test]
fn runs_are_deterministic() {
    let run = || {
        let mut lib = base();
        lib.add(assemble(PICKUP));
        let mut host = RecordingHost::default();
        let mut vm = Vm::new(&mut lib, &mut host);
        let i = new_ref(&mut vm, "pickup");
        let o = vm.deliver(i, "onactivate", vec![Val::None]);
        let log: Vec<String> = vm
            .effects
            .iter()
            .map(|e| format!("{} {:?} {:?} {}", e.native, e.receiver, e.args, e.result))
            .collect();
        (format!("{o:?}"), log, vm.rules.clone())
    };
    assert_eq!(run(), run());
}

#[test]
fn a_missing_script_is_a_named_error() {
    let mut lib = base();
    let mut host = NoHost;
    let mut vm = Vm::new(&mut lib, &mut host);
    assert_eq!(
        vm.instantiate(None, "nowhere"),
        Err(VmError::MissingScript("nowhere".into()))
    );
}

#[test]
fn every_rule_the_interpreter_names_is_registered_pending() {
    for r in rules::ALL {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
    }
    for op in 1..=46u8 {
        let measured = OPCODES[usize::from(op)].is_some();
        assert_eq!(rules::of_opcode(op).is_some(), measured, "opcode {op}");
    }
}

#[test]
fn events_reach_each_instance_with_a_handler_and_the_log_names_visibility_changes() {
    assert_eq!(
        bind::Event::parse("activate"),
        Some(bind::Event::Named("activate", "onactivate"))
    );
    assert_eq!(
        bind::Event::parse("fragment:3"),
        Some(bind::Event::Fragment(3))
    );
    assert_eq!(bind::Event::parse("dance"), None);
    let mut lib = base();
    lib.add(assemble(PICKUP));
    lib.add(assemble("script Quiet extends ObjectReference"));
    let mut host = RecordingHost::default();
    let mut vm = Vm::new(&mut lib, &mut host);
    let o = vm
        .world
        .object("ref", Some("ref:1".into()), &["objectreference"]);
    let a = vm.instantiate(Some(o), "pickup").unwrap();
    let q = vm.instantiate(Some(o), "quiet").unwrap();
    let bound = bind::Bound {
        object: o,
        base_signature: *b"ACTI",
        instances: vec![("pickup".into(), a), ("quiet".into(), q)],
        fragments: None,
        notes: Vec::new(),
    };
    let out = bind::deliver(&mut vm, &bound, &bind::Event::parse("activate").unwrap());
    // The second script has only the root's empty handler.
    assert_eq!(out, [("pickup".to_string(), Outcome::Completed(Val::None))]);
    assert_eq!(vm.effects[0].receiver.as_deref(), Some("<player>"));
    let p = vm.world.player.unwrap();
    assert_eq!(vm.world.objects[p].types, ["actor"]);
    let run = bind::Run {
        bound,
        outcomes: Vec::new(),
        effects: vm.effects.clone(),
        rules: vm.rules.clone(),
        world: std::mem::take(&mut vm.world),
        answers: Default::default(),
        attached: Default::default(),
        attach_notes: Vec::new(),
        quest_jobs: Vec::new(),
        quest_jobs_left: 0,
        event_counts: Default::default(),
    };
    assert_eq!(
        bind::visibility_changes(&run),
        [("ref:1".to_string(), false)]
    );
    assert!(bind::effect_log(&run).contains("objectreference.disable(on <ref>; false) = none"));
}

#[test]
fn a_function_past_its_last_instruction_returns_its_types_default() {
    let mut lib = base();
    lib.add(assemble(
        "script Tail
         func F Float global Int n
         local Bool t
         15 t n 1
         22 t 2
         26 0.5",
    ));
    let f = lib.scripts["tail"].function("", "f").unwrap().clone();
    let mut host = NoHost;
    let mut vm = Vm::new(&mut lib, &mut host);
    assert_eq!(
        vm.run(None, f.clone(), vec![Val::Int(1)]),
        Outcome::Completed(Val::Float(0.5))
    );
    assert_eq!(
        vm.run(None, f, vec![Val::Int(2)]),
        Outcome::Completed(Val::Float(0.0))
    );
    assert!(vm.rules.contains(rules::DEFAULTS) && vm.rules.contains(rules::RETURN));
}
