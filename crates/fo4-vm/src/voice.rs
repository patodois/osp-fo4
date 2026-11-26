//! Spoken lines and their voice files (`spec/voice.md`): which line of a
//! topic a dialogue action plays, the voice file of each of its responses
//! for the speaker's voice type, and the seconds those files declare.
//! Every step is a pending rule (`voice_*`, `scene_dialogue_*`,
//! `info_shared_response_v1`); nothing here is measured in the running
//! game.
//!
//! The seconds of a voice file come from its caller ([`VoiceFiles::add`]):
//! the census and `quest_run` read them from the file's header with
//! `fo4_audio::header_seconds` (`audio_xwma_dpds_seconds_v1`), so this
//! crate needs no audio reader.
use crate::{bind, cond::Truth, rules};
use fo4_core::Budget;
use fo4_plugin::{
    actor::Context,
    info::Info,
    quest::{AliasValue, Quest},
    typed::{FormRef, Store},
    FormKey,
};
use std::collections::{BTreeSet, HashMap};

/// The folders of the voice path convention (`voice_path_convention_v1`).
pub const ROOT: [&str; 2] = ["sound", "voice"];
/// The extension of a voice file.
pub const EXTENSION: &str = ".fuz";

/// A voice file's path read under `voice_path_convention_v1`:
/// `sound/voice/<plugin>/<voice type>/<8 hex digits>_<response>.fuz`,
/// the hex digits the line's object id inside its plugin, the response
/// its TRDA response number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePath {
    pub plugin: String,
    pub voice_type: String,
    pub object_id: u32,
    pub response: u8,
}

/// Reads a mounted path (lower case, `/` separated, as the VFS keys it)
/// under the convention; `None` for any other path, including names with
/// a further suffix after the response number.
pub fn parse_path(key: &[u8]) -> Option<VoicePath> {
    let p = std::str::from_utf8(key).ok()?;
    let parts: Vec<&str> = p.split('/').collect();
    let [a, b, plugin, voice_type, name] = parts.as_slice() else {
        return None;
    };
    if [*a, *b] != ROOT || plugin.is_empty() || voice_type.is_empty() {
        return None;
    }
    let stem = name.strip_suffix(EXTENSION)?;
    let (hex, n) = stem.split_once('_')?;
    if hex.len() != 8 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    if n.is_empty() || n.len() > 3 || !n.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(VoicePath {
        plugin: plugin.to_string(),
        voice_type: voice_type.to_string(),
        object_id: u32::from_str_radix(hex, 16).ok()?,
        response: n.parse().ok()?,
    })
}

/// One voice file of a response: its voice type folder and the seconds
/// its header declares (`None` when the caller could not read them).
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceFile {
    pub voice_type: String,
    pub seconds: Option<f64>,
}

/// The voice files of the mounted archives by line and response.
#[derive(Debug, Default)]
pub struct VoiceFiles {
    files: HashMap<(String, u32, u8), Vec<VoiceFile>>,
    /// Paths added that follow the convention.
    pub named: usize,
}

impl VoiceFiles {
    /// Adds a mounted voice file under the convention; false (and nothing
    /// added) when the path does not follow it.
    pub fn add(&mut self, key: &[u8], seconds: Option<f64>) -> bool {
        let Some(p) = parse_path(key) else {
            return false;
        };
        self.named += 1;
        let v = self
            .files
            .entry((p.plugin, p.object_id, p.response))
            .or_default();
        v.push(VoiceFile {
            voice_type: p.voice_type,
            seconds,
        });
        v.sort_by(|a, b| a.voice_type.cmp(&b.voice_type));
        true
    }
    /// The files of one response of a line, by voice type.
    pub fn get(&self, line: &FormKey, response: u8) -> &[VoiceFile] {
        self.files
            .get(&(line.plugin.to_ascii_lowercase(), line.object_id, response))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

/// The lines (INFO) of every topic (DIAL), in stored order, from the
/// groups of kind 7 that hold them.
#[derive(Debug, Default)]
pub struct Topics {
    pub lines: HashMap<String, Vec<FormKey>>,
}
impl Topics {
    pub fn read(store: &Store) -> Topics {
        let mut t = Topics::default();
        for (o, topic) in store.topic_children() {
            if o.signature != *b"INFO" || o.deleted {
                continue;
            }
            let k = store.overrides().form_key(o.key, store.order());
            t.lines.entry(bind::record_key(&topic)).or_default().push(k);
        }
        t
    }
    pub fn of(&self, topic: &FormKey) -> &[FormKey] {
        self.lines
            .get(&bind::record_key(topic))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

/// What a scene run needs to time its spoken lines.
#[derive(Debug, Default)]
pub struct Voice {
    pub files: VoiceFiles,
    pub topics: Topics,
}

/// The speaker's voice type folder, when the records settle it.
#[derive(Debug, Clone, PartialEq)]
pub enum Speaker {
    /// The voice type's editor id, lower case, and where it came from.
    VoiceType(String, &'static str),
    /// Why it is not settled.
    Unknown(String),
}

fn vtyp_folder(store: &Store, k: &FormKey, b: &Budget) -> Option<String> {
    if store.signature(k) != Some(*b"VTYP") {
        return None;
    }
    let r = store.raw(k, b).ok()??;
    bind::editor_id(&r).map(|e| e.to_ascii_lowercase())
}

/// The VTCK of an NPC_ record, resolved.
fn npc_voice(store: &Store, k: &FormKey, b: &Budget) -> Option<FormKey> {
    if store.signature(k) != Some(*b"NPC_") {
        return None;
    }
    let (r, plugin) = store.raw_with_plugin(k, b).ok()??;
    let s = r.subrecords.iter().find(|s| s.signature == *b"VTCK")?;
    let raw = u32::from_le_bytes(r.data(s).try_into().ok()?);
    store
        .order()
        .remap(plugin, raw, &r.source.file, s.offset)
        .ok()?
}

/// The base (NAME) of a placed reference, resolved.
fn base_of(store: &Store, k: &FormKey, b: &Budget) -> Option<FormKey> {
    let (r, plugin) = store.raw_with_plugin(k, b).ok()??;
    let s = r.subrecords.iter().find(|s| s.signature == *b"NAME")?;
    let raw = u32::from_le_bytes(r.data(s).try_into().ok()?);
    store
        .order()
        .remap(plugin, raw, &r.source.file, s.offset)
        .ok()?
}

/// The voice type of a placed actor (`voice_speaker_voice_type_v1`): the
/// VTCK of its base, when its records settle it.
pub fn reference_speaker(store: &Store, reference: &FormKey, b: &Budget) -> Speaker {
    match base_of(store, reference, b)
        .and_then(|n| npc_voice(store, &n, b))
        .and_then(|v| vtyp_folder(store, &v, b))
    {
        Some(n) => Speaker::VoiceType(n, "the speaker's base"),
        None => Speaker::Unknown("the speaker's base names no voice type".into()),
    }
}

fn key(f: &FormRef) -> Option<&FormKey> {
    f.key.as_ref()
}

/// The voice type of the actor an alias of a quest names, when its record
/// settles it (`voice_speaker_voice_type_v1`): the alias's own VTCK, else
/// the VTCK of the base of its forced reference, else of its unique
/// actor. An alias filled by the world (anything else) leaves it unknown.
pub fn speaker(store: &Store, quest: &FormKey, alias: i32, b: &Budget) -> Speaker {
    let unknown = |w: &str| Speaker::Unknown(w.to_string());
    if alias < 0 {
        return unknown("the action names no alias");
    }
    let Ok(Some((r, plugin))) = store.raw_with_plugin(quest, b) else {
        return unknown("the scene's quest does not read");
    };
    let ctx = Context {
        order: store.order(),
        plugin,
    };
    let Ok(q) = Quest::read(&r, ctx, b) else {
        return unknown("the scene's quest does not read");
    };
    let Some(a) = q.aliases.iter().find(|a| a.id == alias as u32) else {
        return unknown("the quest has no alias of that id");
    };
    let mut forced = None;
    let mut unique = None;
    for v in &a.values {
        match v {
            AliasValue::Form([b'V', b'T', b'C', b'K'], f) => {
                if let Some(n) = key(f).and_then(|k| vtyp_folder(store, k, b)) {
                    return Speaker::VoiceType(n, "the alias's voice type");
                }
            }
            AliasValue::ForcedReference(f) => forced = key(f).cloned(),
            AliasValue::UniqueActor(f) => unique = key(f).cloned(),
            _ => {}
        }
    }
    let via = |npc: Option<FormKey>, how: &'static str| {
        npc.and_then(|n| npc_voice(store, &n, b))
            .and_then(|v| vtyp_folder(store, &v, b))
            .map(|n| Speaker::VoiceType(n, how))
    };
    if let Some(f) = forced {
        if let Some(s) = via(base_of(store, &f, b), "the forced reference's base") {
            return s;
        }
        return unknown("the forced reference's base names no voice type");
    }
    if let Some(u) = unique {
        if let Some(s) = via(Some(u), "the unique actor") {
            return s;
        }
        return unknown("the unique actor names no voice type");
    }
    unknown("the alias is filled by the world")
}

/// How long a line lasts, or why that is undetermined.
#[derive(Debug, Clone, PartialEq)]
pub enum LineTime {
    Seconds(f64),
    Undetermined(String),
}

/// Whether one line's conditions hold, as `eval` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineTruth {
    Holds,
    Fails,
    /// Undetermined, with the condition functions that leave it open.
    Undetermined(BTreeSet<u16>),
}

/// Every line of a topic in stored order with the truth of its conditions
/// (`eval`), for callers that need to know whether the records leave a
/// single line (`scene_line_stage_fields_v1`). `Err` when the topic holds
/// no line or a line does not read.
pub fn line_truths(
    store: &Store,
    voice: &Voice,
    topic: &FormKey,
    eval: &mut dyn FnMut(&Info, usize, &str) -> Truth,
    b: &Budget,
) -> Result<Vec<(FormKey, LineTruth)>, String> {
    let lines = voice.topics.of(topic);
    if lines.is_empty() {
        return Err("the topic holds no line".into());
    }
    let mut out = Vec::new();
    for k in lines {
        let Ok(Some((r, plugin))) = store.raw_with_plugin(k, b) else {
            return Err("a line of the topic does not read".into());
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let Ok(info) = Info::read(&r, ctx, b) else {
            return Err("a line of the topic does not read".into());
        };
        let t = match eval(&info, plugin, &r.source.file) {
            Truth::True => LineTruth::Holds,
            Truth::False => LineTruth::Fails,
            Truth::Undetermined(why) => {
                LineTruth::Undetermined(why.iter().map(|x| x.function).collect())
            }
        };
        out.push((k.clone(), t));
    }
    Ok(out)
}

/// Why [`line_seconds`] leaves a topic's time open when it holds lines
/// and none of them has conditions that hold.
pub const NO_LINE_HOLDS: &str = "no line of the topic has conditions that hold";

/// The line a topic plays and its seconds (`scene_dialogue_line_choice_v1`,
/// `scene_dialogue_line_audio_v1`, `info_shared_response_v1`): the first
/// line in stored order whose conditions hold (`eval` gives their truth;
/// an undetermined one before it leaves the choice undetermined); its
/// responses in stored order (the shared line's, under that line's id,
/// when it has none and names one); for each, the voice file of the
/// speaker's voice type, or the only voice file when the speaker is not
/// settled; the sum of their seconds. `used` collects the rules relied on.
pub fn line_seconds(
    store: &Store,
    voice: &Voice,
    topic: &FormKey,
    speaker: &Speaker,
    eval: &mut dyn FnMut(&Info, usize, &str) -> Truth,
    used: &mut BTreeSet<&'static str>,
    b: &Budget,
) -> LineTime {
    let un = |w: String| LineTime::Undetermined(w);
    let lines = voice.topics.of(topic);
    if lines.is_empty() {
        return un("the topic holds no line".into());
    }
    used.insert(rules::LINE_CHOICE);
    let mut chosen = None;
    for k in lines {
        let Ok(Some((r, plugin))) = store.raw_with_plugin(k, b) else {
            return un("a line of the topic does not read".into());
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let Ok(info) = Info::read(&r, ctx, b) else {
            return un("a line of the topic does not read".into());
        };
        match eval(&info, plugin, &r.source.file) {
            Truth::False => continue,
            Truth::True => {
                chosen = Some((k.clone(), info));
                break;
            }
            Truth::Undetermined(why) => {
                let f: BTreeSet<u16> = why.iter().map(|x| x.function).collect();
                let f: Vec<String> = f.iter().map(|n| n.to_string()).collect();
                return un(format!(
                    "which line of the topic plays is undetermined (function {})",
                    f.join(", ")
                ));
            }
        }
    }
    let Some((line, info)) = chosen else {
        return un(NO_LINE_HOLDS.into());
    };
    line_audio(store, voice, line, info, speaker, used, b)
}

/// The seconds a given line lasts (`scene_dialogue_line_audio_v1`,
/// `info_shared_response_v1`): its responses in stored order (the shared
/// line's, under that line's id, when it has none and names one); for
/// each, the voice file of the speaker's voice type, or the only voice
/// file when the speaker is not settled; the sum of their seconds. `used`
/// collects the rules relied on.
pub fn line_audio(
    store: &Store,
    voice: &Voice,
    mut line: FormKey,
    mut info: Info,
    speaker: &Speaker,
    used: &mut BTreeSet<&'static str>,
    b: &Budget,
) -> LineTime {
    let un = |w: String| LineTime::Undetermined(w);
    if info.responses.is_empty() {
        let Some(s) = info.shared.as_ref().and_then(|s| s.key.clone()) else {
            return un("the line has no response".into());
        };
        used.insert(rules::SHARED_RESPONSE);
        let Ok(Some((r, plugin))) = store.raw_with_plugin(&s, b) else {
            return un("the shared line does not read".into());
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        match Info::read(&r, ctx, b) {
            Ok(i) if !i.responses.is_empty() => {
                line = s;
                info = i;
            }
            _ => return un("the shared line has no response".into()),
        }
    }
    used.insert(rules::VOICE_PATH);
    used.insert(rules::LINE_AUDIO);
    used.insert(rules::DPDS_SECONDS);
    let mut total = 0.0;
    for resp in &info.responses {
        let files = voice.files.get(&line, resp.number);
        if files.is_empty() {
            return un("a response has no voice file".into());
        }
        let file = match speaker {
            Speaker::VoiceType(v, _) => {
                used.insert(rules::SPEAKER_VOICE_TYPE);
                match files.iter().find(|f| &f.voice_type == v) {
                    Some(f) => f,
                    None => return un("no voice file of the speaker's voice type".into()),
                }
            }
            Speaker::Unknown(_) if files.len() == 1 => &files[0],
            Speaker::Unknown(why) => {
                return un(format!(
                    "several voice types hold the line and the speaker's is not settled ({why})"
                ))
            }
        };
        match file.seconds {
            Some(s) => total += s,
            None => return un("a voice file declares no duration".into()),
        }
    }
    LineTime::Seconds(total)
}
