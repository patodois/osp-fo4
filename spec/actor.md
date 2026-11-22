# Placed actors standing in their rest pose, or posed by a clip

First step towards animated actors: a placed actor (ACHR) is followed to a
skeleton and to the models it wears, and drawn standing in the skeleton's
rest pose, skinned once on the CPU. A Havok animation clip named by the
caller can pose it instead (Clip pose, below).

Every layout below was established by measurement on the winning records of
one installation (base game and six expansions, build declared 1.10.163),
with the census method of [plugin.md](plugin.md) and
[record-layouts.md](record-layouts.md): sizes, offsets, value distributions
and the share of four-byte values that resolve to a winning record of the
expected type. No format documentation, schema, editor or third-party reader
was consulted for this step; record and field names are the four-character
signatures stored in the data. Which records the game uses for an actor's
look is behaviour, and each such step relies on a pending rule of
[hypotheses.tsv](hypotheses.tsv); an actor whose look the data does not
settle is left out with its reasons.

Measurement tools (examples of `fo4-unpack`, outputs kept outside the
repository): `record_census` (subrecord sizes and FormID resolution per
record type), `record_dump`, `actor_census` (the tables below),
`skin_census` (skinned NIFs), `nif_probe`, `actor_check` (rest-pose checks of
one cell) and `hkx_census` (animation survey for the next step).

## Record layouts

Counts are over winning, not deleted records.

| Record | Field | Layout | Measurement |
| --- | --- | --- | --- |
| ACHR | NAME | FormID | 4 bytes and resolves to an NPC_ in all 10,391 |
| ACHR | DATA | six f32: position, rotation | 24 bytes in all 10,391; 5,183 of 5,678 exterior placements lie in their cell's XCLC grid square of 4,096 units; all 31,173 angles within 2 pi |
| ACHR | XSCL | f32 | 4 bytes in all 32 |
| NPC_ | ACBS | u32 at 0 (flags), u16 at 14 (template flags); other bytes open | 20 bytes in all 4,363 |
| NPC_ | RNAM | FormID of a RACE | 4,363 of 4,363 |
| NPC_ | TPLT | FormID of an NPC_ or LVLN | 3,402 (2,668 NPC_, 732 LVLN, 2 unresolved) |
| NPC_ | TPTA | 13 FormIDs (NPC_ or LVLN), zero for none | 52 bytes in all 3,281 |
| NPC_ | WNAM | FormID of an ARMO | 1,833 of 1,833 |
| NPC_ | DOFT | FormID of an OTFT | 2,897 of 2,897 |
| NPC_ | PNAM (repeated) | FormID of an HDPT | 23,252 in 2,153 records |
| RACE | WNAM | FormID of an ARMO | 104 of 104 |
| RACE | ANAM | NUL-terminated `.nif` path, one after MNAM and one after FNAM | 216 in 109 races: MNAM then FNAM in 107, MNAM only in 2 |
| RACE | HEAD (repeated) | FormID of an HDPT | 141 in 16 races; every one after an NAM0 marker, then MNAM (66) or FNAM (75), and after an INDX |
| HDPT | MODL | NUL-terminated `.nif` path | 419 of 420 |
| HDPT | PNAM | u32, a small integer (0 to 9) | 420 of 420 |
| HDPT | DATA | u8 | 420 of 420 |
| HDPT | HNAM (repeated) | FormID of an HDPT | 385 in 184 records |
| HDPT | CTDA (repeated) | 32 bytes (conditions; counted, not read) | 40 records, one each |
| HDPT | TNAM, RNAM | FormID of a TXST, of a FLST | 106, 402 |
| OTFT | INAM | FormIDs (ARMO or LVLI) | 4n bytes in all 477; 902 values, 607 ARMO and 295 LVLI |
| ARMO | INDX, MODL pairs | u16, then a FormID of an ARMA | INDX then MODL in all 1,019; 1,387 MODL, all ARMA |
| ARMO | RNAM | FormID of a RACE | 1,019 of 1,019 |
| ARMO, ARMA | BOD2 | u32 word of bits (first four bytes) | 4 bytes in every one |
| ARMA | RNAM | FormID of a RACE | 1,062 of 1,062 |
| ARMA | MODL (repeated) | FormID of a RACE | 2,001 in 681 records |
| ARMA | MOD2, MOD3, MOD4, MOD5 | NUL-terminated `.nif` path | 1,053, 821, 332, 310 |
| LVLN, LVLI | LVLO | u16 at 0, FormID at 4; bytes 2, 3 and 8 to 11 open | 12 bytes in all; LVLN entries resolve to 1,836 NPC_ and 119 LVLN (1 unresolved) |
| LVLN, LVLI | LLCT | u8 count of LVLO | equal in 375 of 375 LVLN and 2,768 of 2,768 LVLI |

Further facts used by the rules:

- **Template flags.** In all 3,281 NPC_ with TPTA, bit k of ACBS u16 at 14
  is set exactly when TPTA slot k is not zero, for all 16 bits (bits 6, 13,
  14 and 15 are never set). 27,160 of 29,298 non-zero slots equal TPLT.
  Which bit governs which property is not established: in templated records
  the own fields are present either way.
- **ACBS bit 0.** Among the 197 untemplated NPC_ whose VTCK is one of the two
  distinct VTYP of their race's VTCK (8 bytes, two FormIDs), bit 0 is set for
  25 of 26 using the second and for none of 171 using the first.
- **Sections.** The MNAM- and FNAM-section paths of RACE differ by one word
  in the NAM1 group's egt paths of 98 races; the second VTYP's editor ID of
  20 races adds the same word to the first; of the 646 ARMA whose MOD2 and
  MOD3 differ, 607 have an `f` in MOD3's differing part and none in MOD2's
  (3 the reverse). 36 races name two different skeletons; in them both MOD2
  and MOD3 models fit the MNAM-section skeleton (229 and 143 skinned shapes
  with every bone found) and not the FNAM one (14 and 9).
- **Addons per armour.** For the ARMO's own RNAM race, 818 ARMO have one
  serving ARMA (RNAM or MODL races), 199 have two to sixteen, 2 none. 1,094
  of 1,387 ARMA BOD2 words have no bit outside their ARMO's word.

Open, kept raw and named in each typed view's `unread`: the other ACBS
bytes, NPC_ NAM4 and NAM6 (f32 near 1) and RACE DATA (200 bytes), SOFT (an
OTFT, 24 NPC_), HDPT TNAM, RNAM, NAM0 and NAM1, LVLD, LVLM and LVLF (one byte
each; LVLD 0 in 366 LVLN and 100 in 9), INDX values, MOD4 and MOD5, RACE
GNAM (a BPTD), ACHR XRGD (1,423 records).

Reader: `fo4_plugin::actor` (`Achr`, `Npc`, `Race`, `Hdpt`, `Otft`, `Armo`, `Arma`,
`Leveled`), each decoding one record with its plugin's FormID context;
wrong sizes, duplicate singletons, an ANAM outside a section and an MODL
without INDX are named errors.

## From a placed actor to what is drawn

`fo4_scene::actor::resolve` takes the ACHR children of a scene's cells
(placed and temporary groups; initially disabled and distant children are
counted, not drawn), in plugin and object-ID order, and for each:

1. **Look.** From the NPC_ (NAME) it gathers, recursively through templates
   (the slots of the set template bits, or TPLT without TPTA) and leveled
   lists (every LVLO entry of an LVLN whose LVLD, LVLM and LVLF are zero),
   every candidate value of race, ACBS bit 0, skin (WNAM, else the race's
   WNAM) and outfit (DOFT). A property with more than one candidate leaves
   the actor out (`npc_appearance_candidates_agree_v1`).
2. **Skeleton.** The race's ANAM of the section that ACBS bit 0 selects
   (`actor_acbs_bit0_second_section_v1`).
3. **Worn armours.** The skin, then the outfit's ARMO entries; a leveled
   item list in an outfit counts only when every entry gives one ARMO
   (`npc_default_outfit_worn_v1`). An outfit that does not resolve leaves
   the actor out, so no bare body is drawn for it.
4. **Addons.** Every ARMA of each worn ARMO whose races include the actor's
   (`armo_race_addons_drawn_v1`), with MOD2 or MOD3 by section; a skin addon
   whose BOD2 shares a bit with a worn outfit armour's BOD2 is hidden
   (`actor_worn_slots_hide_skin_v1`). An addon without the section's model
   leaves the actor out.
5. **Models.** Paths resolve as record models (`Citation::RecordModel`) and
   load into the loader's asset graph. The blockers of the models' closures
   count as for placed references, except those that skinning reads
   (`UnsupportedSkinning` and the `BSSkin::` blocks).
6. **Placement.** ACHR DATA and XSCL as REFR DATA and XSCL; no NPC_ or RACE
   size applies (`achr_placement_as_refr_v1`).
7. **Head.** The head part models of [Heads](#heads), with their own
   blockers and state: a head that is not settled or not ready leaves the
   head out, never the body.

Each actor carries report lines (`notes`): what was hidden or not read, such
as `head: 10 parts`, `outfit: none declared`, or why it is left out. The
head is resolved after the body (Heads, below).

## Rest pose and skinning

Skinned shapes, their skin blocks and per-vertex weights are in
[nif.md](nif.md) (Skinned shapes). `fo4_scene::actor::pose`:

- **Node rotations act on column vectors.** A node maps a child point p to
  `t + s * R * p`, R being its nine stored words as three rows. Over the
  addon models measured against their race skeletons (MOD2 against the
  MNAM-section skeleton), the models' own bone nodes, which sit flat under
  their root, equal the skeleton's composed world translations within 0.01
  units for 111,574 of 118,050 bones under this reading and for 4,217 under
  the reading the static renderer then used (the rows applied to row
  vectors). The static renderer has read the words this way since the
  next step ([nif.md](nif.md), Node rotation words).
- **Skinning.** Vertex position = sum over its non-zero weights of weight x
  (bone's world transform in the pose) x (bone's stored skin-to-bone
  transform) x stored position (`actor_skin_linear_blend_v1`); a vertex
  whose four weights are zero fails its shape.
- **Bind pose check.** With each model's own bone nodes as the pose, the
  composite transform of every bone of a shape is one translation shared
  within 0.01 units by all its bones for 4,585 of the 5,081 shapes whose
  bones are all in the skeleton (174 more within 0.1, 2 spread, 320 rotated
  by more than 0.001). So the blend of the bind pose moves the stored
  vertices rigidly, by that shared translation: (0, -0.88, 120.86) for most
  humanoid addons, zero for most others. The rest pose is the skeleton's: it
  moves a vertex from its bind position by less than 0.1 for 4,105 of those
  shapes and by 10 or more for 287.
- **Draw.** `fo4_render::draw::build_with_actors` draws each skinned shape of
  each part at the actor's placement, after the placed references (instance
  tags continue after theirs). Skinned meshes are kept per model, shape and
  skeleton. Shapes without a skin are counted (`actor_rigid_shapes`), not
  drawn. An actor not drawable at the build's level is an omission with its
  reasons. `fo4_render::draw::build` is unchanged.

Drawing the rest pose is this step's choice, not a claim that the game
shows it.

## Results

**Test cell and its radius-1 neighbours** (the exterior test cell of the README's measurements, `--around 1`): 11
placed actors (3 more initially disabled, none distant), 10 creatures and 1
humanoid. 1 drawn (a creature with its own skin model). Left out: 4 whose
leveled list gives 2 races and 3 skins, 5 whose leveled list gives 4 skins,
1 humanoid whose template chain gives 2 outfits (so not drawn bare). Checks
of the drawn one (three skinned shapes, 51 bones): bind composites shared
within 0.015 units, rotation within 7e-4; the rest pose moves no vertex more
than 0.025 from its bind position; its lowest vertex is 2.21 below the
placement and 30.89 above the collision found straight below it (cell
collision and terrain); no vertices of two of its shapes coincide within
0.05, so it has no seam to check.

**An interior with clothed humanoids** (one of the interiors with the most drawable actors of two or more parts): 23 placed, 10
drawn (body suit and hands, heads not read); 13 left out by model blockers
(missing texture, cloth data). Hands against suit: 202 vertex pairs within
0.035 in the bind pose are within 0.060 after posing; the lowest vertex is
0.36 below the placement, at the floor.

**Every placed actor of the load order** (`actor_census`, A1): 9,341 placed
(1,050 initially disabled, 3 deleted, none distant). 399 resolve and are
drawable under hypotheses; 205 resolve but their models are blocked
(animation controllers 108, unread NIF blocks 92, textures 10); 8,737 are
left out: leveled lists whose LVLD, LVLM or LVLF byte is not zero 4,962,
leveled or template candidates that disagree 3,231, outfits that do not
resolve 445, an addon without the section's model 79, no skin 19, an armour
without an addon for the race 1. Of all placed actors, 432 noted head parts
not read at that step (see Heads) and 349 a skin addon hidden by worn
armour slots.

**Viewer, radius 1** (`--walk --terrain --around 1` at the test cell,
headless, the viewer's build): load 5.93 s without actors and 5.99 s with
them (actor resolution and model reads 0.03 s; the rest is drawing); scene
credits 476.2 and 483.3 MiB; peak resident memory 1,309.3 and 1,323.7 MB.
With `--no-actors` the headless frames and tags are byte-identical to those
of the commit before (an interior, the test cell alone, `--terrain --around
1`, `--walk --terrain --around 2` and a `--path` walk), against a viewer
built from that commit in the same directory: one built elsewhere breaks
coplanar depth ties differently, so it differs in a few pixels on its own.

## Clip pose

`fo4_scene::actor::clip` poses an actor's skeleton by a Havok animation
clip decoded as in [havok-animation.md](havok-animation.md); the viewer's
`--actor-clip ARCHIVE_PATH` uses it. Which clip an actor plays in the game
is behaviour-graph logic that is not read, so the clip is always the one
named by the caller, never chosen.

- **Havok skeleton.** The first `hkaSkeleton` of the `.hkx` file at the
  skeleton NIF's path with `.hkx` for `.nif`
  (`actor_clip_skeleton_beside_nif_v1`, pending). The clip's binding must
  fit it (`Binding::fits`: its name, distinct tracks below the bone count);
  a skeleton it does not fit is left in its rest pose with a report line.
- **Pose.** Each bone's local transform is its track's sample at the clip
  time, or its reference pose when no track drives it; bones compose from
  the root with the quaternion reading measured against the skeleton NIFs.
  In model space, every skeleton NIF node named like a Havok bone takes the
  bone's transform; every other node its parent's pose after its own stored
  local transform (`actor_clip_pose_by_bone_name_v1`, pending). The result
  replaces the rest pose in the skinning above, unchanged.
- **Refused**, each a named error: a file without exactly one binding
  (`ActorClipBinding`), a lossless clip (`ActorClipCompression`), a binding
  whose byte 80 is not 0 (`ActorClipBindingByte80`: 98 of the 102 clips
  whose file name says "additive" store 2 there, and 951 other clips 1 or
  2; what it selects is not established), and a sampled scale whose three
  words differ by more than 0.001 (`ActorClipScale`; the pose uses one
  scale per bone).
- **Draw.** `fo4_render::draw::build_with_posed_actors` skins the actors of
  the posed skeleton assets in their pose and returns their skinned meshes
  (`pose::SkinnedShape`) so that a caller can pose them again; with no pose
  given its draw list is that of `build_with_actors`.

**Checks** with the actor drawn at the test cell (the dog, radius 1; dog
skeleton of 65 Havok bones, all 65 named in its NIF; 3 skinned shapes of
5,034 vertices), over every frame of two clips, with the `actor_clip_check`
example (report kept outside the repository):

| Check | Idle, 4.4 s, 133 frames | Run to stand, 0.633 s, 20 frames |
| --- | ---: | ---: |
| Largest change of a bone's distance to its Havok parent from frame 0, bones whose track has no translation curve | 2.1e-14 | 1.4e-14 |
| Same, bones whose track has a translation curve | 0.150 | 17.79 |
| Lowest skinned vertex minus the placement height, over the clip | -0.84 to -0.75 | -5.03 to 1.21 |
| Lowest Havok bone but the root, minus the placement height | 3.03 to 3.11 | -0.20 to 7.65 |
| Pose and skin one frame on the CPU (release build) | mean 0.040 ms, largest 0.065 ms | mean 0.040 ms, largest 0.067 ms |

In the rest pose the lowest vertex is 2.21 below the placement. The
collision found straight below the placement is 33.1 below it, as in the
rest-pose step: the idle's lowest vertex stays 32.25 to 32.34 above that
collision, so the floor check is against the placement height, which the
frames show at the drawn ground. At frame 0 of the idle, 3 of the 67 named
skeleton NIF nodes are within 0.01 of their rest-pose world position: the
clip moves the body's centre bone by 5.93 from the reference pose and
bends the legs and tail (largest 27.9 at the tail tip), while in local
terms 57 of its 65 tracks keep the reference translation within 0.001
([havok-animation.md](havok-animation.md)).

**Frames.** Headless frames of the dog at 0 and 2.2 s of the idle differ
in 33,863 of 921,600 pixels; the rest pose and the idle at 0 s in 55,917.
Frames are kept outside the repository.

**Window.** The clip loops from `--actor-clip-time`; each frame the main
thread samples the clip, poses the skeleton and skins and replaces the
animated meshes (the loading thread of the moving grid is not involved;
`--actor-clip` is refused with the moving grid, so `--no-stream` is
needed with `--walk --terrain --around N`). At the test cell with `--walk
--terrain --around 1 --no-stream` and a scripted walk (299 frames each, the
viewer's optimised dev build on the 16 GB Mac): posing, skinning and
replacing 5,034 vertices took 0.053 to 0.059 ms per frame on average over
each 60 frames, 0.120 ms at most; the median frame update was 0.490 ms
with the clip and 0.403 ms without, the median frame interval 16.67 ms in
both.

**Without `--actor-clip`** the headless frames and tags of the viewer are
byte-identical to those of 487c57c built in the same path, in eight cases:
an interior without actors, an interior with 10 drawn humanoids, the test
cell alone, with `--terrain --around 1` from two poses (one showing the
dog), `--walk --terrain --around 2`, a `--path` walk, and `--no-actors`.

## Heads

Head parts are drawn unmorphed: the models that the data names, skinned to
the actor's skeleton like an addon, without face morphs, tints, morph
files or texture-set overrides (`npc_head_parts_unmorphed_v1`, pending).
The census is section H of `actor_census` (outputs outside the
repository); the layouts are in the table above.

**What the data says** (winning records):

- **HDPT.** 420 records. PNAM holds 0 (159), 1 (16), 2 (87), 3 (93), 4
  (43), 7 (3), 8 (11) or 9 (8). The 154 parts that another part names in
  HNAM all have DATA bit 3 set and PNAM 0; 2 more have bit 3 set, 264
  neither. 40 parts carry conditions (CTDA), with PNAM 3 (30) or 2 (10).
  Which part a PNAM value is (face, eyes, hair...) is a label at most; the
  code uses only "0" for parts named as extras, and does not use it to
  choose.
- **NPC_ PNAM.** 2,153 NPC_ list 1 to 72 parts, all HDPT; no value but 0 is
  listed twice in one NPC_. 2,210 list none: 1,551 of races without HEAD,
  659 of races with HEAD in one or both sections. The HNAM extras of a
  listed part are listed too in most cases but not all: for listed parts
  with PNAM 2, 3 and 4, 6,070, 2,329 and 1,113 extras are listed and 76, 109
  and 16 not; for PNAM 1, 305 listed and 1,695 not. 1,236 listings name a
  part with conditions.
- **RACE HEAD.** All 141 follow an NAM0 marker, then MNAM (66) or FNAM
  (75), each after an INDX: the race's parts of that section
  (`Race::section_head_parts`).
- **Models.** For each NPC_'s parts (its race's section parts when it
  lists none) against its race section's skeleton, 312 model and skeleton
  pairs: 313 skinned shapes, 297 with every bone named in the skeleton;
  the 16 others name hair cloth bones (`..._Cloth00` and the like) or one
  of 3 other bones the skeleton lacks. Of the 297, the composite of the
  model's own bone nodes and the stored skin-to-bone transforms is one
  translation within 0.01 for 293 (4 rotated: PNAM 7 and one PNAM 4), the
  same (0, -1, 121) as the humanoid addons for 291 of them; the rest pose
  moves a vertex from its bind position by less than 0.1 for 290 and by 10
  or more for 4 (the 3 PNAM 7 shapes and one PNAM 8). Head parts and body
  addons are bound in the same pose.

**Resolution** (`fo4_scene::actor`, after the body):

1. The list: per NPC_ of the template and leveled candidates of step 1
   above, its PNAM parts, sorted and without repeats, or with none its
   race's HEAD of its section. Every candidate must give the same list;
   otherwise `head: N candidate part lists; not drawn`. An empty list (no
   PNAM, no race HEAD: creatures) draws nothing more.
2. HNAM extras that the list does not name are not added.
3. A listed part with conditions leaves the head out (`head: part ...
   has conditions`): whether it shows is decided at run time.
4. Each part's MODL resolves and loads as a record model; the blockers of
   their closure, apart from what skinning reads, and the rule's own
   blocker (`UnverifiedActorHeadParts`) are the head's, not the actor's.
5. **Covering.** The head's main bone is the bone carrying the largest
   sum of vertex weights over the head parts. A worn outfit addon (not the
   skin) with vertices whose largest weight is on that bone covers the
   head, which is then left out (`head: covered by worn armour ...`); what
   shows under it (hair, a face under an open helmet) is not established.
   Over the census below, the covering addons have under 1 % of their
   vertices on the main bone in 2 actors, 1 to 10 % in 2, 10 to 50 % in 18
   and 50 % or more in 27: no threshold separates them, so any vertex
   counts.

**Drawing** (`fo4_render::draw`): after the body, each part's skinned
shapes as an addon's, in the same pose (rest or clip). A head not ready at
the build's level, and each part that fails (a bone the skeleton lacks, a
surface the renderer refuses), is an entry of `Build::actor_head_omissions`
with its causes; the body stays drawn and the other parts too.

**Results.** Over every placed actor (A1), the actor counts are unchanged
(399 drawable under hypotheses). Of the 604 that resolve, 167 have a head
(162 from their PNAM, 5 from their race), 49 are covered by a worn armour,
33 name a part with conditions, 25 have candidates with different lists and
1 a part without a model; the other 329 name no head parts (no PNAM and a race without HEAD). Of the
399 drawable actors, 81 have a head ready under hypotheses (3 from their
race), 15 a head blocked by an unread NIF block of a part (cloth data) and
303 no head drawn. In the interior with clothed humanoids, 8 of the 10
drawn humanoids get a head (9 to 11 parts each, 2 parts of each left out
because the renderer refuses their blend factors); 1 names a part with
conditions and 1 has a part with cloth data. In `actor_check` of that
interior, the few vertices that head and body parts share in the bind pose
(5 to 7 pairs in 4 actors, at most 0.008 apart) are as far apart after the
rest pose. Headless frames against the commit before: that interior differs
in 2,168 of 921,600 pixels; the seven other cases of the previous steps are
byte-identical (frames and tags), and so are all cases without
`--under-hypotheses`. Frames are kept outside the repository.

## Left as found

- The static renderer composed rotated NIF nodes with the other reading of
  the rotation words (above); it now uses this one, after the OBND census
  of [nif.md](nif.md) (Node rotation words), under the pending
  `nif_matrix_runtime_row_bridge_v1`.
- Creature models carry extra skinned shapes that the rest pose draws, for
  example the drawn creature's two shapes named as caps; whether the game
  draws them in this state is not established.
