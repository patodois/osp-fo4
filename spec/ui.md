# Interface movies (`fo4-ui`)

The game's screens (dialogue choices, the wrist device, inventory,
containers, the heads-up display) are Flash movies stored in the interface
archive and in the add-ons' main archives. This page records the first
reading of them: the bounded reader, the census of every movie, a static
reading of how two screens talk to their host, what a host must provide to
show the four dialogue choices the run already produces, and a headless
picture of a movie's first frame. Nothing here runs a movie and nothing has
been checked against the running game: every behaviour is a pending rule in
[hypotheses.tsv](hypotheses.tsv). No game file name, class name, member
name, label or text appears in this repository; they stay in the step's
notes.

## Sources

Format facts, from public documentation:

- Adobe, *SWF File Format Specification*, version 19 (2012),
  <https://www.adobe.com/content/dam/acom/en/devnet/pdf/swf-file-format-spec.pdf>.
  Scope used: the file header (three-byte signature `FWS`, `CWS` for a zlib
  body, `ZWS` for LZMA; version byte; little-endian 32-bit length of the
  whole uncompressed file including the eight header bytes; then the stage
  rectangle, the 8.8 fixed-point frame rate and the frame count); the tag
  header (a 16-bit word, code in the upper ten bits, length in the lower
  six, length 0x3f followed by a 32-bit length); `DefineSprite` (sprite id,
  frame count, then its own tag list ending in `End`); shape records and
  the two fill styles of an edge (fill style 0 on its left, fill style 1 on
  its right as the edge is traced); the matrix and colour transform of a
  placement.
- Adobe, *ActionScript Virtual Machine 2 (AVM2) Overview* (2007),
  <https://www.adobe.com/content/dam/acom/en/devnet/pdf/avm2overview.pdf>.
  Scope used: the multiname kinds and how many stack operands a runtime
  namespace or runtime name takes; the stack effect of the property,
  call and push instructions the look-back follows.
- The `swf` crate 0.4.0 (Ruffle project, MIT OR Apache-2.0,
  [THIRD_PARTY.md](../THIRD_PARTY.md)) parses tags, shapes, texts, fonts,
  sprites and the ABC blocks; its documentation on docs.rs is the only
  Ruffle material consulted. No Ruffle code is copied.

The signatures `GFX` and `CFX` are accepted as uncompressed and zlib bodies
because the `swf` crate accepts them; the census found neither.

## Reader

`fo4_ui::Movie::read(bytes, name, limits, budget)`, or `Movie::open(vfs,
path, ...)` for an archive entry (the archive layer `fo4-unpack` is built
on: `fo4-vfs` over `fo4-archive`):

- Caps (`fo4_ui::Limits`): 64 MiB stored, 128 MiB declared, two million
  tags in the movie, sprites nested four deep, 16 KiB kept per string.
- A zlib body is inflated by `fo4_core::zlib` to exactly the declared
  length, through the caller's `Budget`; the parser's own decompression is
  never used. An LZMA body is refused (`Unsupported`).
- The tag walk is the project's own (`fo4_ui::walk`): each tag's stored
  length is checked before the parser sees the tag; a tag the parser
  refuses is reported (`Movie::tag_errors`) and the walk goes on after its
  stored length; a sprite's tag list is walked once, by the same walk, so a
  refused tag inside a sprite does not lose the rest of it; a stream ending
  inside a tag header is reported with code `fo4_ui::NO_CODE`. Every call
  into the parser runs behind `catch_unwind`: a panic becomes the named
  error `ParserPanic`.
- What it keeps: header fields, the census by tag code (count and body
  bytes), exported symbols, class links (`SymbolClass`), import tags, shape
  statistics (fill styles by kind, line styles, edges), text fields and
  static texts (initial text, variable name, flags), fonts, sprites (frames,
  tags, placements, labels), named placements, frame labels, and every
  ActionScript block as an opaque span with its kind (frame, init, ABC,
  button, clip event), its timeline and, for `DoAbc2`, its name and lazy
  flag.

## Census

`ui_census MANIFEST OUT_DIR` reads every `.swf` entry of the manifest's
mounts: [ui-census.txt](ui-census.txt) (classes and counts only) and, in
the notes, one line per movie with its path. On one installation:

- 344 movies: 304 in the interface archive, 40 in add-on archives; no
  overridden copy, no loose file.
- Every movie is a zlib body (`CWS`) whose declared length equals the
  inflated body; every tag of every movie parses; every movie ends in its
  `End` tag with no byte after it. No tag code outside the parser's table.
- SWF versions 10, 15, 17, 25 and 32; frame rates 24, 28, 30 and 32; ten
  stage sizes, the most common 11,000 by 8,000 twips (244 movies).
- 98 movies hold ActionScript, all of it AVM2: 672 `DoAbc2` blocks, all on
  the root timeline and all lazily initialised, 4.26 MB of bytecode. No
  AVM1 block of any kind.
- No `ExportAssets` entry: symbols reach code through 1,250 class links in
  98 `SymbolClass` tags. 63 movies hold one import tag each, every one with
  no entries (a source only).
- 11,721 shapes (versions 1 to 4), 50 morph shapes, 3,222 sprites, 752 text
  fields (487 marked HTML, 683 without a font id of their own), 2 static
  texts, 28 fonts (all version 3), 57 bitmaps, 3 sounds.

## Host interface (static reading)

`ui_host MANIFEST OUT_DIR LABEL=PATH[@WORD] ...` reads the ABC blocks of
the movies it is given ([ui-host.txt](ui-host.txt), counts and kinds; names
in the notes). The reading (`fo4_ui::abc`) lists every class with its
members and finds the calls that leave the movie by a straight-line
look-back from each property call to the op that pushed its receiver
(`ui_host_calls_static_v1`): `ExternalInterface.call` and `fscommand` with
a constant name, and calls through a member the movie declares untyped or
as `Object`, never gives a value of its own (an empty object, null,
undefined or a method's parameter stored there do not count), never places
by instance name, and calls through. The classes that declare such a
member are the classes the host talks to; their public instance methods,
own and inherited within the block, are the entry points
(`ui_host_entry_public_v1`). `@WORD` also reads the components whose class
name holds that word: the chain of named placements from the root down to
them, their public members, and whether any code in the block names their
text fields.

Two screens were read: the dialogue screen and, for subtitles, the
heads-up screen (the census holds no movie of subtitles alone).

| | Dialogue screen | Heads-up screen |
| --- | ---: | ---: |
| ABC blocks, classes, method bodies | 1, 18, 272 | 1, 198, 1,265 |
| Property call sites (receiver not named by the look-back) | 300 (4) | 887 (8) |
| `ExternalInterface` or `fscommand` calls | 0 | 0 |
| Host-filled members (type `Object`) | 1 | 1 (handed on to a second class) |
| Distinct host functions called through it | 3 (0, 1 and 5 arguments) | 1 (1 argument) |
| Classes the host talks to | 1 | 2 |
| Their public instance methods | 11 (7 with no argument, 4 with two) | 5 (2 with none, 3 with two) |

### The dialogue screen

Read from the counts above and, by hand, from the op listing in the notes:

- The movie builds a list of four choice slots in a fixed order in its
  constructor, each with a fixed controller prompt.
- When the host has filled the member it calls the method that takes no
  argument and announces it; the movie then calls the host's register
  function with five objects: the four slot clips in slot order and one
  animation clip for a speech check.
- An entry point with two arguments (a slot index and a text) sets one
  slot's text; the movie upper-cases it. When slot 3's text is set while the
  movie is hidden, the movie makes itself visible.
- Entry points with no argument show and hide the slots' prompts, enable
  and disable the screen (each plays a labelled frame), and play the speech
  check's animation; on that animation's end the movie calls the host's function
  of no argument.
- An entry point with two arguments (an event name and whether it is a
  press) takes the player's input: four event names map to the four slots
  in slot order, a press marks the slot pressed, a release reports to the
  host's function of one argument the slot's index; it answers whether it
  handled the event.

### Subtitles

The heads-up screen's subtitle component is one class with two public text
fields and one public setter, placed two named placements below the root.
No code in the block names either text field outside the component, and no
code writes their text (`text` or `htmlText` stores whose receiver the
look-back names): the host writes them through the placement chain
(`ui_subtitle_fields_by_path_v1`).

## What a host must provide to show the four choices

`fo4-vm` already holds, for each player dialogue action (scene action kind
3), the four (player topic, answer topic) pairs in
`fo4_vm::scene::PLAYER_CHOICES` order (`ActionInfo::choices`), and the scene
clock waits on the player there unless the caller assumes a choice
(`--assume-player-choice`, `--assume-player-choice-for`). A host that shows
the dialogue screen for such an action must, under the pending rules below:

1. Load the dialogue screen's movie, fill its host member with an object
   that answers the three functions the movie calls (register the five
   clips; a slot released, with its index; the speech check's animation
   ended), and call the announcing entry point.
2. For each slot i in 0..4, give the text of choice i
   (`ui_dialogue_slot_order_v1`, `ui_dialogue_choice_text_v1`) through the
   text entry point, slot 3 last (`ui_dialogue_text_order_v1`), then show
   the prompts and enable the screen.
3. Forward the player's input as the four named events
   (`ui_dialogue_input_events_v1`).
4. On a release of slot i, end the action's wait as the caller's choice i
   for that scene would, at the release's second
   (`ui_dialogue_wait_v1`), then disable the screen.
5. For each spoken line the run plays, write the speaker's name and the
   line's text into the subtitle component's two fields by their placement
   chain (`ui_subtitle_fields_by_path_v1`); the run's timeline already
   carries the speaker and the text of each line.

What is missing for that: a player that runs AVM2 and draws a movie's
display list over time (this step draws one static first frame), a way for
a stepping caller to give `fo4-vm`'s scene clock a choice at run time (today
only the command line's assumptions), and the text of a player line's
prompt, a field `fo4-plugin` does not read yet.

## First frame

`fo4_ui::raster::first_frame(movie, scale, max_pixels, ...)` and the
`ui_frame MANIFEST PATH OUT.png [SCALE]` example draw the root's first
frame: the shapes it places and the first frame of every sprite it places,
with their matrices and colour transforms, over the movie's background
colour, by a minimal scanline fill (non-zero rule over the edges each fill
style bounds, four rows sampled per pixel, curves cut into eight segments).
Presentation only: solid fills as stored, a gradient as the mean of its
colours, a bitmap fill as mid grey; lines, texts, bitmaps placed alone,
morph shapes, buttons, masks, filters and blend modes are left out and
counted. Pictures of the dialogue, message box and heads-up screens and of a
character portrait were made in the notes; none is versioned.

## Limits

- The look-back stops at control flow, `dup`, `swap` and any op it does not
  model; such call sites are counted as unresolved (4 and 8 above) and are
  never classed as host calls.
- The host-member rule is a name rule across the block: two classes with a
  member of the same name are read together.
- The `swf` crate's ABC reader sizes some vectors from counts in the file
  before reading them (method metadata); on a 64-bit machine an absurd count
  reserves virtual memory that is never touched, but the reader cannot cap
  it from outside. The synthetic noise test passes; game blocks are well
  formed.
- Placements' names, frame labels and texts are kept for the notes only.

## Rules

| Rule | Basis | Switch |
| --- | --- | --- |
| `ui_host_calls_static_v1` | static reading; the member rule is the project's own | none: the `ui_host` reading only, no run reads it |
| `ui_host_entry_public_v1` | static reading; prior general knowledge that the interface player lets the host call a movie's methods | none (no host exists yet) |
| `ui_dialogue_slot_order_v1` | the slots' placement names and the choice tags' meanings carry the same four words in the same order (weak) | none (no host exists yet) |
| `ui_dialogue_choice_text_v1` | prior general knowledge that a line stores a short prompt (weak) | none |
| `ui_dialogue_text_order_v1` | the movie's code shows itself on slot 3's text (moderate for the movie, weak for the host) | none |
| `ui_dialogue_input_events_v1` | the movie's code (moderate); the keys behind the four events are unknown | none |
| `ui_dialogue_wait_v1` | the scene clock's existing wait and caller choice (weak) | none |
| `ui_subtitle_fields_by_path_v1` | no code writes the fields (moderate) | none |

No rule has a switch because no code path acts on them yet: the reader and
the census are measurements, and no run, frame or window output changes with
this crate present.
