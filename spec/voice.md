# Spoken lines and their voice files

How a scene's dialogue action reaches the audio it plays, and what in the
voice file says how long that audio lasts, measured on one installation
(the one of [audio.md](audio.md)). The census tool is `voice_census` (an
example of `fo4-vm`), its counts [voice-census.txt](voice-census.txt);
the names behind them go to a file that is never versioned. What a scene
does with the seconds is in [vm.md](vm.md) (Scene clock). Nothing here is
measured in the running game: every reading below is a pending rule in
[hypotheses.tsv](hypotheses.tsv).

## Sources

- **Measurement alone.** No page, tool or source of any implementation
  was opened for this step. The chain action, topic, line, response, file
  was recognised from prior general knowledge and kept only where the
  census, which could have refuted each link, holds it.
- The voice container and its `XWMA` audio half are read by `fo4-audio`
  ([audio.md](audio.md)); the seconds come from `fo4_audio::header_seconds`.
- Topic and line records are read by `fo4-plugin`: the lines of a topic
  from the groups of kind 7 that hold them (`Store::topic_children`), the
  responses, conditions and shared line of a line by
  `fo4_plugin::info::Info`, the speaker alias by `fo4_plugin::quest`.

## The chain

```text
SCEN action kind 0 --DATA--> DIAL (topic)
DIAL --group of kind 7 labelled with it--> INFO (lines, stored order)
INFO --TRDA (one per response), byte 4 = response number--> N
INFO without TRDA --DNAM--> the INFO whose responses it plays
file: sound/voice/<line's plugin>/<voice type>/<line's object id, 8 hex>_<N>.fuz
voice type: the speaker alias's VTCK, else its forced reference's base's VTCK,
            else its unique actor's VTCK; the folder is that VTYP's editor id, lower case
```

| Link | Rule | Measured (counts in [voice-census.txt](voice-census.txt)) |
| --- | --- | --- |
| action to topic | `scen_record_layout_v1` | DATA names a DIAL in 13,503 of the 13,542 dialogue actions (34 name no record, 5 no winning record) |
| topic to lines | `scene_dialogue_line_choice_v1` | lines per action's topic: 1 in 10,218, 2 in 909, 3-19 in 1,184, 20 or more in 21, none in 1,176 (1,153 of these topics store TIFC 0, the rest 1 to 6) |
| line to responses | `voice_path_convention_v1` | every TRDA is 20 bytes (96,725); the byte at offset 4 is distinct within a line in all 86,913 lines with responses, 1 to the count in 86,257, increasing in stored order in 86,554; bytes 5 to 7 zero in 96,524; 15,520 lines have no TRDA |
| shared line | `info_shared_response_v1` | of the 15,520 lines without responses, 9,206 name an INFO in DNAM, and that INFO has a file for every response in 9,133 (37 a response without, 36 no responses) |
| response to file | `voice_path_convention_v1` | 85,445 of the 86,913 lines with responses have a file for every response in their own plugin's folder, 1,468 none; of 1,518 responses without one, 60 have a file of that name in another plugin's folder |
| file to line | `voice_path_convention_v1` | 155,282 of the 158,051 `.fuz` entries under `sound/voice` have the shape (the others: 2,679 named by a word and a one-letter suffix in one folder, not by an id; 65 one folder deeper; 25 with a further suffix after the number); 150,249 of those are named by a response of their line; of the 5,033 that are not, the id names no winning record in 4,471, an INFO without that response number in 376, another record type in 186 |
| folder to voice type | `voice_path_convention_v1`, `voice_speaker_voice_type_v1` | 629 of the 641 voice type folders are the editor id of a winning VTYP (732 VTYP); every plugin folder is a loaded plugin's name; files per response: 1 voice type folder in 66,554 responses, 2 in 21,991, 3 or more in 6,662 |
| speaker to folder | `voice_speaker_voice_type_v1` | the speaker alias settles a voice type in 8,807 dialogue actions (alias VTCK 775, unique actor 8,017, forced reference's base 15); where the topic has files, that voice type's folder holds them in 671 of 682, 7,558 of 7,569 and 14 of 14; not settled in 3,525 (the alias is filled by the world 2,857, forced reference's base without VTCK 279, no alias of that id 250, no alias 130, unique actor without VTCK 9) |

**Coverage per voice type folder** (the share of its files a response of
their line names): 100% in 486 folders, 90-99% in 111, 50-89% in 38,
1-49% in 4, none in 2. The names of the folders with their counts are in
the census's names file.

**Coverage for scene lines**: every response of every line of the topic
has a file for 12,167 of the 12,332 dialogue actions whose topic holds
lines (165 fail); the topic's lines are spread over one voice type folder
in 9,938 actions, two in 791, three or more in 847, none in 756.

## The voice container's header fields

Established from the bytes of the 155,282 conventional voice files (all
read to the last byte by `fo4_audio::Sound::read`, as in
[audio.md](audio.md)):

| Field | Where | Values |
| --- | --- | --- |
| magic, version, lip length | `FUZE` header, 12 bytes | version 1; lip half 0 or 128 to 268,642 bytes, kept raw |
| container of the audio half | RIFF form | `XWMA` in all |
| format tag | `fmt `, u16 at 0 | 0x161 in all |
| channels, sample rate | `fmt `, u16 at 2, u32 at 4 | 1 and 44,100 Hz in 154,738; 2 and 48,000 Hz in 544 |
| average bytes per second | `fmt `, u32 at 8 | 4,000 (mono), 8,000 (stereo) |
| block align (packet size) | `fmt `, u16 at 12 | 1,487 (mono), 2,731 (stereo); the `data` length is a whole number of packets in all |
| bits | `fmt `, u16 at 14 | 16 in all |
| packet table | `dpds`, one u32 per packet | as many words as packets in all ([audio.md](audio.md)) |
| payload | `data` | raw, not decoded |

## Duration

**Reading** (`audio_xwma_dpds_seconds_v1`, prior knowledge of the `dpds`
table): the last `dpds` word counts the 16-bit bytes decoded up to the end
of the last packet, so a file lasts `last dpds / (2 x channels x sample
rate)` seconds. Nothing is decoded.

**What supports it without decoding.** The `dpds` table and the declared
average byte rate are written apart; if the first gives seconds, the stored
`data` bytes must grow with those seconds at about the declared rate.
Least squares over every conventional voice file (part 5 of the census):

| Format | Files | Bytes per second, fitted | Declared | Intercept |
| --- | ---: | ---: | ---: | ---: |
| mono, 44,100 Hz | 154,738 | 4,101.4 | 4,000 | 3,585 bytes (2.4 packets) |
| stereo, 48,000 Hz | 544 | 7,981.7 | 8,000 | 5,385 bytes (2.0 packets) |

Within 5% of the fitted line: 87,775 mono and 289 stereo files; 5-10%:
33,749 and 103; 10-25%: 24,534 and 115; more: 8,680 and 37. The slope
agrees with the declared rate within 2.5%, which the reading predicts and
which nothing forced; the intercept (a near-constant two packets) is why
`data` over the declared rate alone overstates short files (the audio
census found that ratio within 10% of the `dpds` reading in only 6,231
files). The `dpds` steps between packets, in frames of 2 x channels bytes,
are whole frames in every packet, a multiple of 2,048 frames in 782,807 of
1,754,321 packets other than the last, of 512 in 441,128 more, of 128 in
342,260 more.

Seconds by the reading: under 1 s 11,276 files, 1-2 s 34,993, 2-5 s
73,897, 5-10 s 32,752, 10-30 s 2,336, 30 s or more 28.

**What is still missing.** That the seconds so read are the seconds the
game plays is not measured: no audio was decoded (the codec is not
implemented, [audio.md](audio.md)) and the running game was not timed. The
lip half was searched for a stored duration (an f32 within 5% of the
reading in its first 64 bytes): none at a fixed offset (155,168 of 158,051
files have none there). Where the reading leaves a line's time
undetermined, or to run without it (`quest_run --no-line-audio`), the
caller may supply `--assume-line-seconds X`, counted as an assumption.

## A line's time in a scene

`fo4_vm::voice::line_seconds`, under `scene_dialogue_line_audio_v1`:

1. The line is the first of the topic's lines in stored order whose
   conditions hold, evaluated by `fo4_vm::cond` in the run's state with
   the scene's quest as owner (`scene_dialogue_line_choice_v1`); an
   undetermined line before it leaves the time undetermined, named by
   function index. Line flags (ENAM) are not read.
2. Its responses in stored order, or the responses of the line its DNAM
   names when it has none (`info_shared_response_v1`).
3. For each response, the file of the speaker's voice type when the
   records settle it (`voice_speaker_voice_type_v1`), else the only file
   when one voice type folder holds it; several folders and no settled
   speaker leave it undetermined.
4. The sum of their seconds; no pause between responses, and the action's
   DMIN and DMAX (1 and 10 as f32 in nearly every dialogue action) are not
   read.

Player dialogue (kind 3) is never timed by the records; under the caller
assumption `--assume-player-choice N` the action lasts the player's line
of choice N and then the answer line it pairs with
(`scene_player_dialogue_choice_v1`): the topics PTOP, NTOP, NETO, QTOP
pair with NPOT, NNGT, NNUT, NQUT, which is also their stored order in
2,929 of the 3,159 player dialogue actions. The player's voice type is
not in the records, so the player's line is timed only when one folder
holds it or under `--assume-line-seconds`.

## Prior knowledge used

- That a scene's dialogue action plays a topic's line, a line is made of
  responses each with its own audio file, voice files are grouped by voice
  type and named by the line's id and the response number, a line can
  share another's responses, and an alias's voice type comes from its
  actor.
- That the `dpds` words count decoded bytes; that the average byte rate
  of the format chunk is the nominal bit rate.
- That the letters of the player dialogue topics mean positive, negative,
  neutral and question, the N-prefixed ones the answers.
- That the TIFC word of a topic counts its lines (used only to describe
  the topics without lines).

## What is not known

- Which line of a topic plays when several hold, and what the line flags
  do; what a dialogue action with a topic without lines does (1,176
  actions).
- The meaning of the TRDA bytes other than the response number, of DMIN
  and DMAX, and of the 2,679 files named by a word.
- The lip half's layout.
- Every duration as played: one timed recording per rule would decide it
  (the live measurements in [hypotheses.tsv](hypotheses.tsv)).
