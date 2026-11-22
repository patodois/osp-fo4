# Enable parents (XESP)

A placed record may name another placed record as its enable parent. This
file records the subrecord's layout, a census of it over one installation,
the pending rules under which the scene decides a placement's presence
through its chain of parents, what that changes in the frames and how a
script's Enable or Disable now reaches the children. Nothing here has been
measured in the running game: every behaviour below is a `pending` rule in
[hypotheses.tsv](hypotheses.tsv). Names, FormIDs and cell names stay out of
this file; the tools write them only to unversioned outputs.

## Layout

`XESP`: a FormID (4 bytes, little endian, in the file's master context),
one flag byte and three bytes. Source: the published record definitions
cited in [plugin.md](plugin.md) ("XESP FormID, flags byte, three bytes
reserved"). `fo4_plugin::enable::read` reads it from any record to the last
byte: two XESP in one record (`RepeatedEnableParent`) or another size
(`EnableParentSize`) are named errors; the three bytes after the flags are
kept and not interpreted. `fo4_plugin::enable::chain` follows the parents
from a placed record to a record without XESP, and ends by name at FormID 0,
a FormID the load order cannot resolve, a key with no winning record, a
cycle or after 64 links. The REFR typed view kept reading XESP as before;
the ACHR view keeps it among its unread subrecords, and the chain reader
reads it from the record.

## Census (`enable_parent_census`)

`enable_parent_census MANIFEST OUT.txt` (an example of `fo4-unpack`) reads
every winning placed record of the load order (2,001,939, 21 s, peak RSS
0.65 GB) and writes counts only: [enable-parent-census.txt](enable-parent-census.txt).

- 39,461 placed records carry an XESP: 36,814 REFR, 2,497 ACHR, 93 PHZD,
  57 PGRE. Every one is 8 bytes, never two in a record.
- Flag byte: 0x00 37,110, 0x01 2,227, 0x02 56, 0x03 68; no other value.
  Bytes 5 to 7 are zero in 39,446; `FF FF FF` in 14 and `FF 00 00` in 1,
  all with flag byte 0.
- Every FormID resolves to a winning record: REFR 39,419, ACHR 39, PHZD 3;
  none is deleted; 19,763 parents carry the header flag 0x800.
- Every parent sits in a cell's persistent group (kind 8), 39,461 of
  39,461; children sit in persistent (4,292) and temporary (35,169) groups.
  Parent in the child's cell 17,719, in another cell 21,742 (exterior child
  and exterior parent 20,337, exterior child and interior parent 1,186,
  interior child and exterior parent 145, both interior 74).
- 1,693 distinct parents, 135 of which carry an XESP themselves; children
  per parent: one 408, two 291, three or four 296, five to nine 291, ten to
  19 154, 20 to 49 103, 50 to 99 75, a hundred or more 75.
- Chains: depth 1 38,101, depth 2 1,331, depth 3 29; 39,458 end at a record
  without XESP (REFR 39,399, ACHR 56, PHZD 3), 3 in a cycle.
- Flag bits used along a chain: none 36,396, bit 0 2,927, bit 1 50, both 85.
- Start state under the rules below: enabled 18,517, disabled 20,941; 866
  children carry the header flag 0x800 themselves (366 of them with a chain
  that says enabled), a case the rules do not decide.

## Rules

| Rule | Covers | Raised as |
| --- | --- | --- |
| `xesp_child_follows_parent_v1` | a record with an XESP is present (drawn, colliding) exactly when its parent is enabled | `RuntimeEnableParent` `XESP` |
| `xesp_initial_state_chain_v1` | the start state of a record without XESP is its header flag 0x800 (set: disabled); with XESP, its parent's through the chain | `RuntimeEnableParent` `XESP.initial` |
| `xesp_flag_bit0_opposite_v1` | flag bit 0 inverts the link | `XESP.opposite`, when a link of the chain has it |
| `xesp_flag_bit1_not_presence_v1` | flag bit 1 does not change presence | `XESP.bit1`, when a link has it |
| `xesp_script_change_propagates_v1` | a script's Enable or Disable reaches the children transitively; a record's own change wins over its parent's | the viewer's `--script-event` |

Undecided chains raise a `RuntimeEnableParent` blocker no rule covers:
`XESP.cycle`, `XESP.unresolved` (FormID 0, unresolved, missing record),
`XESP.too_deep`, `XESP.flags` (a bit other than 0 and 1) and
`XESP.own_flag` (a record of the chain other than its root carries its own
flag 0x800 beside its XESP: whether the own flag or the parent decides is
not settled).

## Scene (`fo4_scene::enable`)

`Loader::instance` reads the chain of every placed REFR with an XESP and
raises the blockers above in place of the one `RuntimeEnableParent` blocker
of before; `Instance::enable` keeps the chain and its start state
(`Presence`). The placement is ready only at the hypotheses level (every
rule pending); the draw build and the collision placement then leave out an
instance whose chain says disabled (`DisabledThroughEnableParent`), after
the readiness check, so nothing changes without `--under-hypotheses`.
Placed actors (ACHR) get the same blockers and `Actor::enable`; an actor
whose chain says disabled is not drawn. References that carry the flag
0x800 stay in `Scene::disabled`, as before, whatever their chain says.

### Coverage (`coverage_census`, every cell of the load order)

Against the census of the commit before (spec/coverage.md):

| Outcome | Before | After |
| --- | ---: | ---: |
| drawn only under hypotheses | 527,821 | 538,375 |
| hidden by `xesp_child_follows_parent_v1` | — | 6,857 |
| omitted | 446,615 | 429,204 |

Placements with an XESP after this step, by start state and outcome:
references whose chain says enabled: 10,665 drawn under hypotheses, 5,994
omitted for other reasons, 559 hidden as editor markers; chain says
disabled: 7,636 not drawn through the chain, 10,802 omitted for other
reasons; not decided: 462 omitted, 8 hidden as editor markers. Actors:
enabled 45 drawn, 143 omitted; disabled 135 not drawn through the chain,
156 omitted; not decided 2 omitted. So 10,665 references go from not drawn
to drawn, and 135 actors from drawn or omitted to not drawn: 111 were drawn
before and 24 were omitted by the draw build (`ActorSkinBoneMissing`),
which now does not reach them. No reference goes from drawn to not drawn:
every reference with an XESP was omitted before. Frames without hypotheses
are unchanged (below).

Test views (the five of [coverage.md](coverage.md)): lobby and vault hold no
placement with an XESP; the exterior test cell gains 1 placement drawn under
hypotheses (one chain says disabled: the large mist effect, still omitted for
its animation controllers), radius 1 gains 9 and radius 2 gains 12; no
placement of the test views is not drawn through a chain alone.

## Script events (`fo4-view --script-event`, `script_run --census`)

The viewer now applies, after the effect log's direct changes, every
change to the instances whose chain names a changed reference
(`vm_event::propagate`, `propagate_actors`), printing each with its start
and new state. `script_run --census` reads the children of every changed
reference transitively (an index of every placed REFR and ACHR by the
parent its XESP names) and counts, for each run, how many children change
state and whether the scene would draw them (`enable children:` lines in
`vm-census.txt`; names and the cell and index to reproduce each case in
`vm-enable-children.txt`, not versioned).

- The test cells (the lobby, the vault and the exterior test cell with
  radius 2): one child changes (enabled to disabled), an editor marker; no
  drawable placement changes.
- Every placed scripted reference of the installation: of the runs whose
  handlers all complete, 26 reach at least one child (3,906 reach none);
  41 REFR go from disabled to enabled (13 drawable under every pending
  rule), 40 from enabled to disabled (4 drawable), 54 keep their state, 3
  are not decided; 7 ACHR go from disabled to enabled (6 drawable). Six runs
  change at least one drawable placement: four change one, one changes 8
  (6 actors appear, a reference appears and one disappears) and one
  changes 11 (10 references appear, one disappears).

Three cases rendered headless (1280 x 720, `--under-hypotheses`, before and
after the event, same camera; frames kept outside the repository):

| Case | Event | Changes through the chain | Pixels that differ |
| --- | --- | --- | ---: |
| exterior, terminal fragment | `fragment:2` | 10 MISC references appear, one STAT disappears (opposite flag) | 48,377 |
| interior, lighting terminal | `fragment:1` | a ceiling light STAT disappears (opposite flag), others not drawable | 6,939 |
| interior, button activator | `activate` | a small pipe STAT appears | 2,831 |

In each pair the tags show the change on the children's instances only.
The data host's `IsDisabled` still answers from the header flag alone, not
from a chain; a script that reads a child's state is answered as before.

## Limits

- One installation; no measurement of the running game.
- A child with its own flag 0x800 is not decided and stays out at every
  level, as an initially disabled reference did before.
- The viewer's moving grid and walk mode use the same draw build and
  collision, so they leave out what a chain disables; only
  `--script-event` changes a state after load.

## Provenance

Recognised from prior knowledge, kept only as pending rules that the
measurements above could refute: that XESP makes a record's presence follow
its parent's, that flag bit 0 means "opposite of the parent" and bit 1 a fade
choice ("pop in"), and that a script's Enable or Disable of a parent reaches
its children. The layout itself is the published record definition already
cited in [plugin.md](plugin.md). No editor, editor source, script source or
game executable was opened.
