// Surface of the GPU viewer. Mirrors the per-sample steps of the CPU
// reference rasterizer (crates/fo4-render/src/raster.rs, `Raster::triangle`):
// UV transform, mip level from the screen footprint, bilinear sample with
// wrap, an effect's palette lookups, vertex colour, the frame's light and
// fog on a lit surface, opacity, the palette alpha, an effect's falloff,
// the colour clamped to 0..1, alpha test, zero-alpha skip. Blending is set
// on the pipeline, not here. The `sky` entry point draws the light's sky by
// view direction (`fo4_render::render_lit`'s background).
import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_functions::get_tag;
import bevy_pbr::render::mesh_view_bindings::view;
@if(TARGET_SRGB) import bevy_render::color_operations::srgb_to_linear;

struct SurfaceParams {
    uv_scale: vec2<f32>,
    uv_offset: vec2<f32>,
    test_ref: f32,
    opacity: f32,
    // NiAlphaProperty function 0..7, 8 = CPU reference threshold (>=), 9 = no test.
    test_function: u32,
    skip_zero_alpha: u32,
    // 1: the texture's alpha is replaced by 1 (stored BC1 blocks declared
    // without alpha, terrain textures), as the CPU's decoded texels hold it.
    opaque_texture: u32,
    // 1: `falloff` holds an effect falloff (`fo4_render::Falloff`): start
    // and stop angle (cosines), start and stop opacity.
    falloff_on: u32,
    // Palette lookups (`fo4_render::Palette`): bit 0 colour, bit 1 alpha.
    palette_lookups: u32,
    falloff: vec4<f32>,
    // `fo4_render::Draw::colour`: an RGB multiplier after the vertex colour
    // (an effect colour the game clock drives); 1 otherwise.
    tint: vec4<f32>,
    // Bit 0: lit by `lights`.
    mode: u32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: SurfaceParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var diffuse: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var diffuse_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var palette: texture_2d<f32>;
// `scene::light_words`: rows of four values in game axes (Z up).
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var<storage, read> lights: array<vec4<f32>>;

const LIGHT_HEADER_ROWS: u32 = 14u;

// Bevy's axes (Y up) to the game's (Z up): (x, y, z) -> (x, -z, y).
fn to_game(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(v.x, -v.z, v.y);
}

// `fo4_scene::light::Sky::colour` at elevation sine `s`.
fn sky_colour(s: f32) -> vec3<f32> {
    let zenith = lights[11u].xyz;
    let lower = lights[12u].xyz;
    let horizon = lights[13u].xyz;
    let at = lights[12u].w;
    if s <= 0.0 {
        return horizon;
    }
    if s < at {
        return horizon + (lower - horizon) * (s / at);
    }
    return lower + (zenith - lower) * min((s - at) / (1.0 - at), 1.0);
}

// `fo4_scene::light::Light::at` at the game point `p` with unit normal `n`.
fn light_at(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var c = lights[0u].xyz;
    if lights[3u].w > 0.5 {
        let w = n * n;
        c = c + w.x * select(lights[3u].xyz, lights[4u].xyz, n.x < 0.0);
        c = c + w.y * select(lights[5u].xyz, lights[6u].xyz, n.y < 0.0);
        c = c + w.z * select(lights[7u].xyz, lights[8u].xyz, n.z < 0.0);
    }
    if lights[1u].w > 0.5 {
        c = c + lights[1u].xyz * max(dot(n, lights[2u].xyz), 0.0);
    }
    let count = u32(lights[0u].w);
    for (var k = 0u; k < count; k = k + 1u) {
        let a = lights[LIGHT_HEADER_ROWS + 2u * k];
        let v = a.xyz - p;
        let d = length(v);
        if d < a.w && d > 0.0 {
            let facing = max(dot(n, v) / d, 0.0);
            c = c + lights[LIGHT_HEADER_ROWS + 2u * k + 1u].xyz * ((1.0 - d / a.w) * facing);
        }
    }
    return c;
}

// `Texture::sample_clamped`: the base level, bilinear, the coordinates kept
// within the edge texel centres, where a repeating sampler reads the edge
// texel alone (a clamp to the edge).
fn palette_sample(uv: vec2<f32>) -> vec4<f32> {
    let half_texel = 0.5 / vec2<f32>(textureDimensions(palette, 0));
    let at = clamp(clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), half_texel, 1.0 - half_texel);
    return textureSampleLevel(palette, diffuse_sampler, at, 0.0);
}

// `Falloff::factor` at the absolute cosine `c`.
fn falloff_factor(c: f32) -> f32 {
    let f = params.falloff;
    if f.x == f.y {
        return f.z;
    }
    let t = clamp((c - f.x) / (f.y - f.x), 0.0, 1.0);
    return f.z + (f.w - f.z) * t;
}

// `Alpha::passes` with x = alpha * 255 and r = test reference.
fn passes(alpha: f32) -> bool {
    let x = alpha * 255.0;
    let r = params.test_ref;
    switch params.test_function {
        case 0u: { return true; }
        case 1u: { return x < r; }
        case 2u: { return x == r; }
        case 3u: { return x <= r; }
        case 4u: { return x > r; }
        case 5u: { return x != r; }
        case 6u: { return x >= r; }
        case 7u: { return false; }
        case 9u: { return true; }
        default: { return x >= r; }
    }
}

struct SkyOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

// The sky (`gpu::spawn_sky`): the elevation sine of the view direction
// (Bevy's +Y is the game's up), at the far depth (0, reverse Z).
@fragment
fn sky(in: VertexOutput) -> SkyOutput {
    let d = normalize(in.world_position.xyz - view.world_position);
    var colour = vec4<f32>(sky_colour(d.y), 1.0);
    @if(TARGET_SRGB)
    colour = vec4<f32>(srgb_to_linear(colour.rgb), colour.a);
    return SkyOutput(colour, 0.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The face normal from the screen derivatives of the world position
    // (taken here, in uniform control flow).
    let face = cross(dpdxFine(in.world_position.xyz), dpdyFine(in.world_position.xyz));

    // The CPU transforms UVs per vertex and interpolates; the transform is
    // affine, so transforming the interpolated UV is the same value.
    let uv = in.uv * params.uv_scale + params.uv_offset;
    // Level of detail: 0.5 * log2(max(|det J| * width * height, 1)) with J the
    // screen derivatives of the UV, rounded to the nearest level and clamped
    // to the levels uploaded (the base plus its listed coarser mips).
    let dx = dpdxFine(uv);
    let dy = dpdyFine(uv);
    let size = vec2<f32>(textureDimensions(diffuse, 0));
    let footprint = abs(dx.x * dy.y - dx.y * dy.x) * size.x * size.y;
    let lod = 0.5 * log2(max(footprint, 1.0));
    let level = min(floor(lod + 0.5), f32(textureNumLevels(diffuse) - 1u));
    var color = textureSampleLevel(diffuse, diffuse_sampler, uv, level);
    if params.opaque_texture == 1u {
        color.a = 1.0;
    }
    var vertex = vec4<f32>(1.0);
    @if(VERTEX_COLORS)
    vertex = in.color;
    // Palette lookups: U from the texel's green (colour) or alpha (alpha),
    // V from the opacity times the vertex alpha.
    var palette_alpha = -1.0;
    if params.palette_lookups != 0u {
        let v = clamp(params.opacity * vertex.a, 0.0, 1.0);
        if (params.palette_lookups & 1u) != 0u {
            color = vec4<f32>(palette_sample(vec2<f32>(color.g, v)).rgb, color.a);
        }
        if (params.palette_lookups & 2u) != 0u {
            palette_alpha = palette_sample(vec2<f32>(color.a, v)).a;
        }
    }
    @if(VERTEX_COLORS)
    color = color * vertex;
    color = vec4<f32>(color.rgb * params.tint.rgb, color.a);
    // A lit surface: the colour times the light at the point, then the
    // fog by distance from the eye (`light_sum_clamped_v1`); the normal is
    // the face normal turned toward the eye.
    if (params.mode & 1u) != 0u {
        let p = to_game(in.world_position.xyz);
        let eye = to_game(view.world_position);
        var n = normalize(to_game(face));
        if dot(n, eye - p) < 0.0 {
            n = -n;
        }
        if lights[2u].w > 0.5 {
            color = vec4<f32>(color.rgb * light_at(p, n), color.a);
        }
        if lights[9u].w > 0.5 {
            let near = lights[10u].x;
            let far = lights[10u].y;
            let distance = length(eye - p);
            var t = 0.0;
            if far > near {
                t = clamp((distance - near) / (far - near), 0.0, 1.0);
            } else if distance >= far {
                t = 1.0;
            }
            let f = max(min(t, lights[10u].z), 0.0);
            color = vec4<f32>(color.rgb * (1.0 - f) + lights[9u].xyz * f, color.a);
        }
    }
    color.a = color.a * params.opacity;
    if palette_alpha >= 0.0 {
        color.a = palette_alpha;
    }
    // Falloff: the cosine between the interpolated normal and the direction
    // to the eye, by its absolute value.
    if params.falloff_on == 1u {
        let n = normalize(in.world_normal);
        let e = normalize(view.world_position - in.world_position.xyz);
        color.a = color.a * falloff_factor(abs(clamp(dot(n, e), -1.0, 1.0)));
    }
    // The frame holds colour within 0..1 (an 8-bit target clamps the
    // output before blending; the CPU clamps here).
    color = vec4<f32>(clamp(color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
    if !passes(color.a) {
        discard;
    }
    if params.skip_zero_alpha == 1u && color.a == 0.0 {
        discard;
    }
    // The blend reads the alpha within 0..1 after the test saw the product
    // (an opacity above 1; the 8-bit target would clamp it anyway).
    color.a = clamp(color.a, 0.0, 1.0);
    @if(INSTANCE_TAG) {
        // Byte k of the tag in channel k: an RGBA8 unorm target stores the
        // tag's little-endian bytes, the layout of `Frame::instance_bytes`.
        // The tag is the instance's `MeshTag` (`Draw::instance`).
        return unpack4x8unorm(get_tag(in.instance_index));
    } @else {
        // The CPU writes its working values as stored bytes. On an sRGB
        // target the hardware encodes on store, so decode first to store the
        // same bytes.
        @if(TARGET_SRGB)
        color = vec4<f32>(srgb_to_linear(color.rgb), color.a);
        return color;
    }
}
