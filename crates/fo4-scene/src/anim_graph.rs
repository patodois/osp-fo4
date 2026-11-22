//! The animation graph of a model, as far as a state machine of Gamebryo
//! sequence states needs (`spec/havok-behavior.md`): the model's
//! `BSBehaviorGraphExtraData` names a behaviour project file (under
//! `meshes`), the project names one character file, the character names
//! the behaviour graph file (both relative to the project's folder), and
//! the graph's root state machine plays the model's own controller
//! sequences, whose text keys are read with their times.
//!
//! The chain of files is a measured convention (`anim_event_census`); what
//! the game does with it (`anim_graph_sequence_events_v1` and the rules of
//! the consumer in `fo4-vm`) is pending. Nothing here runs the graph.
use fo4_core::{path_key_bytes, Budget, Game, Limits};
use fo4_havok::{character_behavior, only_object, project_characters, Container, Machine};
use fo4_nif::{Data, Nif, Value};
use fo4_vfs::Vfs;

/// One controller sequence of a NIF with its text keys, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub name: String,
    /// `Cycle Type` as stored (2 in every sequence whose keys the opening
    /// reaches; its meaning is not read here).
    pub cycle: u32,
    pub frequency: f32,
    pub start: f32,
    pub stop: f32,
    /// (time, text) in stored order.
    pub keys: Vec<(f32, String)>,
}

/// What a model's NIF says about animation: the behaviour graph file its
/// `BSBehaviorGraphExtraData` names, and its controller sequences.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NifAnimation {
    pub graph_file: Option<String>,
    pub sequences: Vec<Sequence>,
}

fn field<'a>(f: &'a [fo4_nif::Field], name: &str) -> Option<&'a Value> {
    f.iter().find(|x| x.name == name).map(|x| &x.value)
}

fn text(n: &Nif<'_>, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::StringIndex(i)) => n
            .strings
            .get(*i as usize)
            .map(|s| String::from_utf8_lossy(s).into_owned()),
        _ => None,
    }
}

fn float(v: Option<&Value>) -> Option<f32> {
    match v {
        Some(Value::FloatBits(b)) => Some(f32::from_bits(*b)),
        _ => None,
    }
}

/// Reads the behaviour graph file and the controller sequences of a
/// decoded NIF. A sequence whose text-key link names no
/// `NiTextKeyExtraData` keeps no keys.
pub fn nif_animation(n: &Nif<'_>) -> NifAnimation {
    let mut out = NifAnimation::default();
    let fields = |i: usize| match n.blocks.get(i).and_then(|b| b.decoded.as_ref()) {
        Some(Data::Fields(f)) => Some(f.as_slice()),
        _ => None,
    };
    let kind = |i: usize| {
        n.blocks
            .get(i)
            .and_then(|b| n.block_types.get(b.type_index as usize))
            .map(String::as_str)
    };
    for i in 0..n.blocks.len() {
        let Some(f) = fields(i) else { continue };
        match kind(i) {
            Some("BSBehaviorGraphExtraData") if out.graph_file.is_none() => {
                out.graph_file = text(n, field(f, "Behavior Graph File"));
            }
            Some("NiControllerSequence") => {
                let mut keys = Vec::new();
                if let Some(Value::Ref(r)) = field(f, "Text Keys") {
                    let r = usize::try_from(*r).ok();
                    if let Some(k) = r.filter(|r| kind(*r) == Some("NiTextKeyExtraData")) {
                        let kf = fields(k).unwrap_or(&[]);
                        let mut time = None;
                        for x in kf {
                            match x.name {
                                "Time" => time = float(Some(&x.value)),
                                "Text" => {
                                    if let (Some(t), Some(s)) =
                                        (time.take(), text(n, Some(&x.value)))
                                    {
                                        keys.push((t, s));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                out.sequences.push(Sequence {
                    name: text(n, field(f, "Name")).unwrap_or_default(),
                    cycle: match field(f, "Cycle Type") {
                        Some(Value::U32(c)) => *c,
                        _ => u32::MAX,
                    },
                    frequency: float(field(f, "Frequency")).unwrap_or(f32::NAN),
                    start: float(field(f, "Start Time")).unwrap_or(f32::NAN),
                    stop: float(field(f, "Stop Time")).unwrap_or(f32::NAN),
                    keys,
                });
            }
            _ => {}
        }
    }
    out
}

/// The archive key of a text relative to a root folder (`meshes`) or to
/// another file's folder.
pub fn key_under(folder: &str, rel: &str) -> Option<String> {
    let joined = format!("{folder}/{}", rel.replace('\\', "/"));
    let k = path_key_bytes(joined.as_bytes()).ok()?;
    String::from_utf8(k).ok()
}

/// The folder of an archive key.
pub fn folder_of(key: &str) -> &str {
    key.rsplit_once('/').map_or("", |(d, _)| d)
}

/// A model's graph: the files of the chain, the root state machine and
/// the NIF's sequences.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelGraph {
    pub nif: String,
    pub project: String,
    pub character: String,
    pub behavior: String,
    pub machine: Machine,
    pub sequences: Vec<Sequence>,
}
impl ModelGraph {
    /// The sequence of a name (the NIF stores each once in every model the
    /// opening reaches; the first is taken).
    pub fn sequence(&self, name: &str) -> Option<&Sequence> {
        self.sequences.iter().find(|s| s.name == name)
    }
}

fn container_text<'a>(
    c: &Container<'a>,
    class: &str,
    file: &str,
    b: &Budget,
) -> Result<String, String> {
    let i = only_object(c, class).ok_or_else(|| format!("{file}: not one {class}"))?;
    let t = match class {
        fo4_havok::PROJECT_STRINGS => {
            let all = project_characters(c, i, file, 0, b).map_err(|e| e.to_string())?;
            match all.as_slice() {
                [one] => *one,
                [] => return Err(format!("{file}: the project names no character")),
                _ => return Err(format!("{file}: the project names several characters")),
            }
        }
        _ => character_behavior(c, i, file, 0).map_err(|e| e.to_string())?,
    };
    Ok(String::from_utf8_lossy(t).into_owned())
}

/// Reads the graph of a model from its record's model text (MODL, under
/// `meshes`). Every step that fails is a text naming the file and why.
pub fn model_graph(
    vfs: &Vfs,
    model: &str,
    limits: &Limits,
    b: &Budget,
) -> Result<ModelGraph, String> {
    let nif = key_under("meshes", model).ok_or("the model path is not a valid archive path")?;
    let bytes = vfs
        .read(&nif, &b.child())
        .map_err(|e| format!("{nif}: {}", e.code))?;
    let n = Nif::read(&bytes, &nif, Game::Fallout4, limits, &b.child())
        .map_err(|e| format!("{nif}: {}", e.code))?;
    let anim = nif_animation(&n);
    let graph = anim
        .graph_file
        .ok_or_else(|| format!("{nif}: no behaviour graph extra data"))?;
    let project = key_under("meshes", &graph).ok_or("the graph file is not a valid path")?;
    let read = |key: &str, b: &Budget| -> Result<Vec<u8>, String> {
        vfs.read(key, b).map_err(|e| format!("{key}: {}", e.code))
    };
    let pb = read(&project, &b.child())?;
    let pc = Container::read(&pb, &project, 0, &b.child()).map_err(|e| e.to_string())?;
    let rel = container_text(&pc, fo4_havok::PROJECT_STRINGS, &project, b)?;
    let character =
        key_under(folder_of(&project), &rel).ok_or("the character path is not valid")?;
    let cb = read(&character, &b.child())?;
    let cc = Container::read(&cb, &character, 0, &b.child()).map_err(|e| e.to_string())?;
    let rel = container_text(&cc, fo4_havok::CHARACTER_STRINGS, &character, b)?;
    let behavior = key_under(folder_of(&project), &rel).ok_or("the behaviour path is not valid")?;
    let bb = read(&behavior, &b.child())?;
    let bc = Container::read(&bb, &behavior, 0, &b.child()).map_err(|e| e.to_string())?;
    let machine = Machine::read(&bc, &behavior, 0, &b.child()).map_err(|e| e.to_string())?;
    Ok(ModelGraph {
        nif,
        project,
        character,
        behavior,
        machine,
        sequences: anim.sequences,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_join_folders_and_normalise() {
        assert_eq!(
            key_under("meshes", "Effects\\A\\B.hkx").as_deref(),
            Some("meshes/effects/a/b.hkx")
        );
        assert_eq!(folder_of("meshes/a/b.hkx"), "meshes/a");
        assert_eq!(
            key_under("meshes/a", "Behaviors\\X.hkx").as_deref(),
            Some("meshes/a/behaviors/x.hkx")
        );
        assert_eq!(key_under("meshes", "..\\x.hkx"), None);
    }
}
