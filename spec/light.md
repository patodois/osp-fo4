# Light and sky of a frame

What a frame's light is read from, under which pending rule, and what the
renderers do with it (`fo4_scene::light`, `fo4_render::render_lit`, the
viewer's `surface.wesl`). Every reading is behind its own switch, off by
default; with every switch off nothing below changes and the earlier frames
compare byte for byte. The census of what the data stores along the
forced opening's run is [light-census.txt](light-census.txt)
(`light_census`, stored numbers and counts only).

Nothing here was measured in the running game. The light model is the
simplest the data allows; the game's own shading (normal maps, specular,
shadows, image space, tone mapping, exposure, bloom) is not reproduced.

## Data read

| Data | Where | Basis |
| --- | --- | --- |
| Interior ambient, directional colour and rotation, fog near and far, fog colours, fog maximum, the `Inherits` word | CELL.XCLL, LGTM.DATA (and LGTM.DALC) through the layout fields | sourced layouts ([record-layouts.tsv](record-layouts.tsv)), word-to-label correspondences ([record-flag-words.tsv](record-flag-words.tsv)) |
| Placed lights: position, radius, colour | the scene's instances whose base is a LIGH with DATA | sourced LIGH.DATA layout |
| A reference's radius adjustment | REFR.XRDS (f32) | sourced layout; its use is a rule |
| A light's fade value | LIGH.FNAM (f32) | sourced layout; its use is a rule |
| The exterior's climate | WRLD CNAM (a CLMT) | `fo4_plugin::typed::Wrld::climate` |
| The climate's weathers and times | CLMT WLST (12-byte entries), TNAM (6 bytes) | `fo4_plugin::weather::Climate`: measured shape, no source |
| A weather's colour table, directional ambient, fog values, image spaces | WTHR NAM0 (four-byte colours, one row per DALC slot), DALC (32 bytes, the LGTM.DALC layout), FNAM (f32 values), IMSP (FormIDs) | `fo4_plugin::weather::Weather`: measured shape; DALC's layout sourced through LGTM.DALC |

Measured shapes (light census, whole load order): every CLMT WLST is a
whole number of 12-byte entries whose first word resolves to a WTHR and
third word to zero or a GLOB; every TNAM is 6 bytes. Weathers store 4 or 8
DALC; NAM0 holds the DALC count times 17 or 19 colours (4 slots 17 rows: 4
weathers; 8 slots 17 rows: 2; 8 slots 19 rows: 101); the readers refuse
none. The fourth byte of every colour in the run's weather is zero.

Not read: image space records (the interior cell names one, every weather
slot names one), god rays, lens flares, cloud textures and their speeds,
the sun and moon textures, region weather data (the run's regions hold
none), XLIG, a light's flags beyond the printed type labels counted by the
census, spot FOV and near clip, flicker, the falloff exponent field.

## Rules and switches

| Switch (render and fo4-view --headless) | Rule | Reading |
| --- | --- | --- |
| `--light-cell` | `cell_light_ambient_directional_v1` | interior: ambient added everywhere, directional colour by the cosine along `(cos XY cos Z, cos XY sin Z, sin XY)` |
| `--light-inherits` | `cell_light_template_inherits_v1` | interior: the fields the `Inherits` word names from the LTMP template |
| `--light-points` | `light_point_linear_cutoff_v1` | every placed light a point: colour times `(1 - d/R)` times the cosine, zero at and beyond R |
| `--light-reference-radius` | `light_reference_radius_added_v1` | R is the base's radius plus the reference's XRDS |
| `--light-fade` | `light_fade_scales_colour_v1` | colour times the base's FNAM |
| `--light-fog` | `cell_fog_linear_v1` | interior fog toward Fog Color Near by `min(clamp((d - near)/(far - near)), max)` |
| `--sky-weather` | `weather_sky_rows_v1` | exterior background: rows 8, 7, 0 of the weather's table at elevation sines 0, 0.25, 1 |
| `--light-weather` | `weather_ambient_cube_v1` | exterior ambient cube from the slot's DALC; a normal toward +axis reads the axis's `-` entry |
| `--fog-weather` | `weather_fog_rows_v1` | exterior fog toward row 1 with FNAM near, far, maximum (values 1, 2, 7 by day, 3, 4, 8 at night) |
| (any weather switch) | `climate_single_weather_v1` | the climate's one weather; with several, `--assume-weather N` is the caller's |
| (any weather switch, with `--assume-hour H`) | `climate_hour_slot_v1` | TNAM's first four bytes as tens of minutes: slots 3, 0, 1, 2 between them; no blending |
| (any switch that lights) | `light_sum_clamped_v1` | colour times the summed light, then fog, clamped to 0..1; no tone mapping |

The hour and the weather are the caller's (`--assume-hour H`,
`--assume-weather N`): a run timeline records seconds, not the game's hour.
The data stores one candidate: a global the notes name holds 11, which
`--assume-hour 11` reads as slot 1 (`spec/light-census.txt`, last section);
that the run starts at that hour is not read.

Where the data stops deciding:

- the falloff curve, how the terms combine and the tone mapping: chosen
  as the simplest readings (spec/light-measurement.md has the bench that
  would settle the falloff);
- the direction the two rotation angles give;
- which rows of the weather's table are sky and fog, the order of its
  slots and the unit of the climate's times: prior general knowledge, with
  the data hints the rule rows give;
- the sun's direction: the data stores times of day and textures, not a
  path; no sun or moon light is drawn;
- which weather applies when a climate lists several (two climates of the
  load order), and whether a region overrides it.

## What the renderers do

`fo4_scene::light::frame` reads the scene's cell (interior or exterior by
its kind) and returns a `Light` (ambient, an ambient cube, a directional
light, points, fog, sky, the rules and one report line per reading;
`light` lines in the overlay). The CPU's `render_lit` lights the draws the
caller marks: in the `render` example and the viewer every draw of the
build and the terrain but effect shader surfaces (`Build::effect_draws`);
the timeline's markers are not lit. Per sample: the face normal of the
triangle turned toward the eye (presentation: vertex normals and normal
maps are not read), the light at the sample's point (`Light::at_some`, with
only the points whose sphere reaches the draw's bounds, which gives the same
sum), the colour multiplied before the opacity, then the fog by distance
from the eye, then the clamp as before. The sky fills the background by
each pixel's view direction (`fo4_render::pixel_elevation`) before
anything is drawn, so blended surfaces blend over it; tags stay the
background's.

The viewer reads the same `Light` from a storage buffer
(`scene::light_words`) bound to every surface material; a lit surface is a
surface of its own (`SurfaceParams::lit`); the face normal comes from the
screen derivatives of the world position; the sky is drawn by a camera
ordered before the colour pass on the same target (a cube around the eye
whose material colours each pixel by its view direction), the colour pass
keeping it instead of clearing. The tag pass is unchanged.
