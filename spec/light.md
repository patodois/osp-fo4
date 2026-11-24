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

## Exposure, image space and vertex normals (the seen step)

Lit frames washed out: 379 placed lights summed in the vault and nothing mapped the sum to the display. The
image space records (IMGS) the frame names, the interior cell's XCIM or the weather slot's IMSP entry, hold
nine f32 in HNAM, three in CNAM, four in TNAM, a DNAM and a texture path ([seen-census.txt](seen-census.txt),
section 4: every word stored, ranges over the 334 records). The data gives no labels: the readings below take
them from prior general knowledge, each behind its own switch of the CPU render and the headless viewer, off
by default:

| Switch | Rule | Reading |
| --- | --- | --- |
| `--exposure-eye` | `frame_exposure_seen_light_v1` | a lit colour times one over the light nine in ten of the lit triangles in view stay at or below (`fo4_render::seen_lit_points`: centroids in the reference frame, at most 20,000, each with its face normal toward the eye; `Light::expose_seen`), after the light and before the fog. No image space word is used: the measure is chosen |
| `--exposure-white` | `image_space_hdr_white_v1` | after the fog, x maps to x (1 + x / W^2) / (1 + x), W the HNAM word 5 (1 in the opening's interiors, which makes the map the identity; 1.5 to 1.6 by day outside) |
| `--exposure-cinematic` | `image_space_cinematic_tint_v1` | then CNAM's saturation, brightness and contrast, and TNAM's tint amount and colour (neutral in the opening's interiors and day slots) |
| `--light-vertex-normals` | `light_vertex_normals_v1` | the build keeps every rigid mesh's vertex normals and a lit draw is lit with the interpolated normal turned toward the eye (skinned and posed meshes keep the face normal) |

An exterior frame of a climate with several weathers needs `--assume-weather N` for the image space as for the
weather's light. In the vault at 560 s the exposure measures 1.529 (median 0.804) and scales lit colours by
0.654: the sign reads, the textures show; the CPU render and the viewer agree (coverage 1.000000, same tag on
every pixel, mean colour difference under one step with the vertex normals on). Not read: bloom, eye
adaptation over time, depth of field (DNAM), the colour lookup texture (TX00), the sun's and sky's scales.

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

## The play window (the looks step)

The window's scripted play mode (`fo4-view --play-run ... --play-input FILE`) takes the light switches and the
caller's `--assume-hour` and `--assume-weather` (not `--exposure-eye`: the window's camera moves): each cell the
window loads gets its light as the headless frame does (`load::cell`, `scene::convert_lit`), its light buffer is
the one every material binds, and the sky is drawn by the same `sky` entry point on a cube kept about the camera.
In the opening at hour 11 and weather 0 (the light step's frames' assumptions) the exteriors show the sky's
gradient; the prewar and postwar vaults (379 and 817 placed lights as points) wash out toward white without an
exposure, and the window drops to about 16 and 10 fps there at 2560x1440: every light is summed for every pixel.
