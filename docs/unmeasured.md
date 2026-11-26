# What the play preset assumes (not measured)

The play preset (`apps/fo4-view/presets/play-opening.args`, read with `--args`) turns on rules of
[spec/hypotheses.tsv](../spec/hypotheses.tsv) whose status is `pending`: they are **our assumptions, not real
and not measured game behaviour**. They let the opening be played through, and each names the measurement of the
running game that could refute it. Nothing they produce may be called faithful to the game before that
measurement is recorded and the rule's status changed (AGENTS.md, Rules of evidence). The same holds for the
pad's key map below and for every viewer policy the preset uses.

## The preset's switches and their pending rules

One row a rule, in the preset's order; "What it assumes" is the first sentence of the rule's assertion, cut. The
table is checked against the preset and the registry by a test of the viewer
(`cli::tests::the_unmeasured_page_lists_the_presets_rules`): a switch of the preset missing from this page, a
rule a preset comment names that the table lacks, or a rule here that is not pending in the registry fails it.

| Switch | Rule | Status | What it assumes |
| --- | --- | --- | --- |
| `--walk-speed` | `walk_speed_walk_v1` | pending | Walking moves the player at 100 world units per second on level ground (placeholder) |
| `--play-pick-bounds` | `play_activate_object_bounds_v1` | pending | The activation ray also meets the box of the object bounds (OBND, six i16: low and high corner in model units) of each placed reference of the walked ... |
| `--play-pick-bounds-activatable` | `play_activate_bounds_not_static_v1` | pending | The object bounds boxes the activation ray meets leave out those of references whose base is a static (STAT), a static collection (SCOL) or a movable ... |
| `--play-load-doors` | `play_load_door_activation_v1` | pending | Activating a placed door whose base is a door and whose XTEL names a placed door (read as world_load_door_xtel_v1 reads it) puts the walker, at the ... |
| `--play-live-world` | `play_live_collision_follows_run_v1` | pending | The play mode's walking world follows the quest run's state at each of the walker's slices as a timeline frame at that second does ... |
| `--play-elevator-door` | `play_elevator_end_load_door_v1` | pending | When a moving part that carried the walker reaches its track's end with the walker standing on it, the load door nearest the walker's feet among the ... |
| `--play-elevator-held-pose` | `play_elevator_held_pose_v1` | pending | Under play_elevator_run_sequence_v1 a node of a named reference's model that a controller sequence controls through an NiTransformInterpolator whose ... |
| `--play-elevator-rigid` | `play_elevator_rigid_v1` | pending | Under play_elevator_run_sequence_v1 and in place of its translation reading (and of play_elevator_held_pose_v1), a named reference's collision ... |
| `--play-elevator-door-onward` | `play_elevator_end_door_onward_v1` | pending | Under play_elevator_end_load_door_v1 the load doors whose XTEL leads into the space the walker left by its last change of space (a load door crossed, ... |
| `--play-elevator-door-landing` | `play_elevator_end_landing_v1` | pending | Under play_elevator_end_load_door_v1 a ride that carried the walker whose last key passes while the walker is in the air (it lost the moving piece ... |
| `--play-held-doors` | `play_door_closed_blocked_holds_v1` | pending | A placed door among the walking world's loaded references (a reference whose base is a DOOR record, with object bounds) that the run's scripts last ... |
| `--play-held-doors-at-load` | `play_door_closed_at_load_v1` | pending | A placed door that the person opens (a reference --play-activate-opens names) and whose reference stores no ONAM subrecord (spec/record-layouts.tsv, ... |
| `--play-door-open-collision` | `play_door_collision_open_pose_v1` | pending | A loaded door whose collision the readiness refuses (its controllers, record fields or materials: no door's collision was placed before) stands in ... |
| `--play-door-toggle` | `play_door_activation_toggles_v1` | pending | A loaded door whose collision stands in its open pose (play_door_collision_open_pose_v1) is open for the held doors' reading from the load of its ... |
| `--play-door-toggle-blocked` | `play_door_toggle_blocked_unchanged_v1` | pending | The person's activation of a door the held doors' reading shows open (play_door_activation_toggles_v1) whose activation the run's scripts last ... |
| `--play-door-stored-open` | `play_door_stored_open_v1` | pending | A loaded door with a held door box (play_door_closed_blocked_holds_v1) whose collision is not in the walking world and whose reference stores ONAM ... |
| `--play-marker-boxes` | `play_collision_marker_boxes_v1` | pending | The box of each collision marker (walk_collision_marker_box_v1) whose base carries record flag 0x02000000, placed enabled with no enable parent, ... |
| `--play-marker-box-layers` | `play_collision_marker_box_layer_passed_v1` | pending | A collision marker's box that play_collision_marker_boxes_v1 would place stays out of the walking world when its reference stores an XTRI whose word ... |
| `--play-collision-off-chain` | `collision_model_blockers_off_chain_v1` | pending | A blocker raised in a model file on a block that none of the model's placed collision bodies reads (the bodies read the node chain from a root to ... |
| `--play-collision-reference-alpha` | `refr_xalp_collision_unchanged_v1` | pending | A placed reference's XALP subrecord (two bytes whose layout this project has not established; xEdit's published field name is an alpha of the ... |
| `--play-collision-child-flag-agrees` | `xesp_child_flag_agrees_chain_v1` | pending | A placed record below the root of its enable parent chain that carries its own flag 0x800, in a chain whose start state under ... |
| `--play-collision-static-fields` | `refr_static_fields_collision_unchanged_v1` | pending | On a placed reference whose base is a static (STAT), the subrecords XACT, XLRT, XOWN and ONAM (xEdit's published names: action flag, location ref ... |
| `--play-pick-first-met` | `play_activate_ray_first_met_v1` | pending | The activation's ray (play_activate_collision_ray_v1, with play_activate_object_bounds_v1) orders what it meets by whether the eye stands inside it, ... |
| `--play-action-after-offset` | `caller_action_after_keeps_offset_v1` | pending | A caller's timed action held until a stage (--action-after) keeps its offset from that stage: when the stage is done, each action the gate holds ... |
| `--play-action-after-offset` | `play_activate_written_second_v1` | pending | The walker's activation or open of a reference whose caller's timed activation or open the play mode leaves out (--play-activate-person, ... |
| `--play-player-moveto` | `play_player_moveto_v1` | pending | A MoveTo the run calls on the player with three zero offsets and a target the world model settles (a placed reference's stored place, or a place the ... |
| `--play-player-sits` | `play_player_sit_furniture_v1` | pending | A sit the run gives the player (fo4_vm::sit::sit: a script's activation of a furniture reference read as a sit under its own switches, or the ... |
| `--timeline-target-rotation` | `timeline_actor_target_rotation_v1` | pending | An actor a run timeline places somewhere other than its stored position faces as the one placed reference of the loaded cells whose stored position ... |
| `--timeline-child-flag-agrees` | `xesp_child_flag_agrees_chain_v1` | pending | A placed record below the root of its enable parent chain that carries its own flag 0x800, in a chain whose start state under ... |
| `--timeline-nowait` | `timeline_nowait_visibility_v1` | pending | A run's ObjectReference.EnableNoWait and DisableNoWait change a placed reference's presence in a timeline frame as Enable and Disable do under ... |
| `--actor-missing-bone-bind` | `actor_missing_bone_bind_pose_v1` | pending | A bone a skinned shape of an actor's model names that the actor's skeleton does not hold is skinned at its bind transform: its matrix is the ... |
| `--actor-missing-bone-nearest` | `actor_missing_bone_follows_nearest_v1` | pending | Under actor_missing_bone_bind_pose_v1, a bone a skinned shape names that the actor's skeleton lacks is skinned with the composite matrix (the ... |
| `--actor-start-clip` | `char_graph_start_state_clip_v1` | pending | A standing actor that nothing has acted on plays the clip its race's behaviour graph reaches from its root by: each state machine's start state ... |
| `--actor-subgraph-keywords` | `char_graph_swap_subgraph_keywords_v1` | pending | A swap generator (BSBehaviorGraphSwapGenerator, which stores no link in any of the 47) plays the behaviour file of one subgraph entry of the actor's ... |
| `--actor-clip-folder` | `char_graph_clip_folder_order_v1` | pending | A clip generator's text (Animations\...\NAME.hkt) names the archive file NAME.hkx in the first of the subgraph entry's SAPT folders (under meshes) ... |
| `--actor-subgraph-flag-one` | `char_graph_swap_subgraph_flag_one_v1` | pending | A race whose subgraph entries (its own, or those of the race SRAC names) hold no entry of flag word 0 without target keywords plays, at a swap ... |
| `--actor-model-graph-idle` | `actor_model_graph_start_no_sequence_v1` | pending | A part model of a timeline actor (its skin or an outfit piece) that names a behaviour graph (BSBehaviorGraphExtraData) whose root state machine's ... |
| `--skinned-models-own-nodes` | `model_skinned_own_nodes_every_placement_v1` | pending | Every placed reference of a timeline frame's loaded cells that the draw refuses with a skinned shape among its reasons (UnsupportedSkinning, ... |
| `--skinned-models-ragdoll-left-out` | `refr_ragdoll_pose_unread_left_out_v1` | pending | A placement the skinned own-nodes reading (model_skinned_own_nodes_every_placement_v1) would add to the play cast is left out when its reference ... |
| `--light-cell` | `cell_light_ambient_directional_v1` | pending | An interior cell's surfaces are lit by the cell's stored Ambient Color (added everywhere) and its Directional Color along one direction: the unit ... |
| `--light-inherits` | `cell_light_template_inherits_v1` | pending | An interior cell that names a lighting template (LTMP, an LGTM) takes from the template's DATA the fields its own XCLL Inherits word names by printed ... |
| `--light-points` | `light_point_linear_cutoff_v1` | pending | A placed light (a reference whose base is a LIGH with DATA, not left out by the enable parent rules) lights a surface point at distance d below its ... |
| `--light-reference-radius` | `light_reference_radius_added_v1` | pending | A placed light whose reference stores XRDS has the radius its base stores plus the XRDS value (an adjustment, not a replacement) |
| `--light-fade` | `light_fade_scales_colour_v1` | pending | A placed light's colour is multiplied by its base's fade value (LIGH FNAM, an f32) before it lights anything |
| `--light-fog` | `cell_fog_linear_v1` | pending | An interior cell's lit surfaces are mixed toward its Fog Color Near by the fraction min(clamp((d - Fog Near) / (Fog Far - Fog Near), 0, 1), Fog Max) ... |
| `--sky-weather` | `weather_sky_rows_v1` | pending | An exterior frame's background (where no surface is drawn) is a gradient by the sine s of the view direction's elevation: the weather's colour row 8 ... |
| `--light-weather` | `weather_ambient_cube_v1` | pending | An exterior frame's lit surfaces receive the weather's directional ambient (DALC, read with the sourced LGTM.DALC layout) of the slot ... |
| `--fog-weather` | `weather_fog_rows_v1` | pending | An exterior frame's lit surfaces are mixed toward the weather's colour row 1 at the slot climate_hour_slot_v1 gives by the fraction min(clamp((d - ... |
| `--assume-hour` | `climate_hour_slot_v1` | pending | The caller's hour (--assume-hour H) reads one slot of the weather's tables: the climate's TNAM first four bytes are times of day in tens of minutes ... |
| `--assume-weather` | `climate_single_weather_v1` | pending | An exterior cell's worldspace climate (WRLD CNAM) that lists one weather (one WLST entry) has that weather |
| `--exposure-eye` | `frame_exposure_seen_light_v1` | pending | A lit colour is multiplied, after the light and before the fog, by one over the light nine in ten of the triangles the eye sees stay at or below: the ... |
| `--exposure-eye-floor` | `frame_exposure_seen_light_floor_v1` | pending | Under frame_exposure_seen_light_v1 a view whose light nine in ten of the seen lit triangles stay at or below is under 0.25 ... |
| `--exposure-eye-visible` | `frame_exposure_seen_visible_v1` | pending | Under frame_exposure_seen_light_v1 the light is measured on what the eye sees instead of on the centroids of the lit triangles in view: at the centre ... |
| `--draw-child-flag-agrees` | `xesp_child_flag_agrees_chain_v1` | pending | A placed record below the root of its enable parent chain that carries its own flag 0x800, in a chain whose start state under ... |
| `--draw-platforms` | `play_platform_drawn_v1` | pending | Each reference the play run names as a platform (--play-elevator) that stands in the loaded cell's space (in a worldspace, inside the horizontal box ... |
| `--draw-held-doors` | `play_door_drawn_as_held_v1` | pending | Each loaded door is drawn by its model as stored whatever its render readiness, as play_platform_drawn_v1 draws a named platform (shapes the draw ... |
| `--draw-static-fields` | `refr_static_fields_render_unchanged_v1` | pending | On a placed reference whose base is a static (STAT), the subrecords XACT, XLRT, XOWN and ONAM (xEdit's published names: action flag, location ref ... |
| `--draw-static-leaves` | `play_static_leaf_two_pose_v1` | pending | A static whose model's one controller is a free-running, clamped transform controller whose first key is the stored pose and whose keys turn one node ... |
| `--draw-reference-alpha` | `refr_xalp_render_unchanged_v1` | pending | A placed reference's XALP subrecord (two bytes whose layout this project has not established; xEdit's published field name is an alpha of the ... |
| `--draw-scripted-as-stored` | `refr_script_render_as_stored_v1` | pending | The record blocker a placed reference's scripts raise (RuntimeCondition on its VMAD: its state may change at run time) is set aside for its drawing: ... |
| `--draw-activator-fields` | `acti_fields_render_unchanged_v1` | pending | The record blockers of an activator base's (ACTI) unread PNAM, FNAM and ATTX subrecords (xEdit's published names: marker colour, flags, activate text ... |
| `--draw-furniture-fields` | `furn_fields_render_unchanged_v1` | pending | The record blockers of a furniture base's (FURN) unread PNAM, FNAM, MNAM, SNAM and WBDT subrecords (xEdit's published names: marker colour, flags, ... |
| `--draw-world-persistent` | `world_persistent_refs_drawn_in_square_v1` | pending | The placed references (REFR, not actors) of a worldspace's cells' persistent groups whose stored position lies in a grid square the window loads (the ... |
| `--draw-activator-parts` | `acti_unmoved_parts_drawn_v1` | pending | An activator (ACTI) placement refused at render readiness for model blocks no usable rule covers (a behaviour graph, controllers and their sequences, ... |
| `--draw-static-parts` | `static_unmoved_parts_drawn_v1` | pending | A placed static (STAT) with one model that render readiness refuses only for model blocks no usable rule covers (a behaviour graph, particle systems ... |
| `--draw-platform-skins` | `play_platform_skin_follows_parts_v1` | pending | In the play window, a named platform's skinned shape whose bones go with more than one of its moving parts (the parts play_elevator_rigid_v1 reads ... |
| `--draw-platform-nearest-part` | `play_platform_shape_nearest_part_v1` | pending | In the scripted play window a named platform's drawn shape whose node chain's deepest node that a sequence sets holds no moving part (no collision ... |
| `--draw-cast-on-platforms` | `play_cast_stands_on_platform_v1` | pending | In the play window, an actor of the run is drawn at its place's x and y with its feet at the highest top, under that x and y, of a named platform's ... |
| `--draw-cast-keeps-its-part` | `play_cast_keeps_its_part_v1` | pending | In the play window, under play_cast_stands_on_platform_v1, an actor of the run that stood the frame before on a named platform's part keeps standing ... |
| `--draw-cast-keeps-clear` | `play_cast_kept_clear_of_walker_v1` | pending | In the play window an actor of the run whose drawn body at the eye's height comes nearer the eye than its half width plus a capsule radius (a placeholder) is drawn moved out of that reach from every direction, eased round the eye instead of jumping; the run is unchanged |
| `--actor-head-slots` | `npc_head_hidden_by_head_slot_v1` | pending | A worn outfit armour hides what of an actor's head its BOD2 word names, in place of the bone-weight covering clause of npc_head_parts_unmorphed_v1: ... |
| `--actor-head-conditions` | `npc_head_part_conditions_run_stages_v1` | pending | A listed head part (HDPT) with conditions is drawn with the head when its one condition list holds under the condition evaluator's pending rules ... |
| `--actor-unread-texture-slots` | `actor_texture_missing_unread_slot_v1` | pending | A placed or run actor's models are read under texture_missing_unread_slot_v1 as a placed reference's are: a texture file no mounted archive or loose ... |
| `--actor-template-bits` | `npc_template_bits_traits_inventory_v1` | pending | A templated NPC_ (ACBS u16 at 14 not zero) takes its traits (race, ACBS bit 0, skin and head part list) through TPTA slot 0 when bit 0 is set, its outfit through slot 8 and its keywords through slot 12 ... (the bit-to-property mapping is prior knowledge, not read from data) |
| `--actor-leveled-seed` | `npc_leveled_entry_seeded_pick_v1` | pending | Each leveled list an actor's chain reaches yields one entry, picked by a hash of the caller's seed (1 in the preset), the placed reference and the list: the look drawn is the seed's, never the game's |

`--under-hypotheses` sets the rule level to hypotheses: every pending rule the code reads by level
(`fo4_scene::RuleLevel`) applies, besides those the switches above turn on, and the run is no claim about the
game beyond its sourced facts. `--walk-speed 300` gives `walk_speed_walk_v1`'s speed the caller's value (three
times the rule's placeholder), a choice for playing, not a measurement.

The preset's other switches belong to no rule of their own: they choose what the window loads and how the play
mode runs (viewer or play-mode policy, the caller's values): `--walk`, `--terrain`, `--around`, `--no-stream`,
`--play-keys`, `--play-stand`, `--play-reach`, `--play-follow-grid`, `--play-slice`, and `--timeline-cast` (the
run's actors drawn as their characters; how they stand and move is the rules of the `--timeline-*` and
`--actor-*` rows and of the hypotheses level).

## The pad (`--play-pad`)

The pad's key map (spec/play.md, The person's input, The pad) is viewer policy and no rule: the dead zones (0.24
on the left stick, 0.20 on the right), the eight sectors of the left stick and their 0.05 hysteresis, the run
from 0.90 of full tilt (off below 0.80), the look rates (120 degrees a second to turn, 75 to look up or down, by
the square of the deflection), A to activate, Menu to pause and View or B held one second to end. A partial tilt
walks and a full one runs at the walker's two speeds; the stick's speed between them is not the game's measured
behaviour, and nothing about how the game itself reads a pad was measured.
