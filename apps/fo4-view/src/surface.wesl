// Unlit surface of the GPU viewer. Mirrors the per-sample steps of the CPU
// reference rasterizer (crates/fo4-render/src/raster.rs, `Raster::triangle`):
// UV transform, mip level from the screen footprint, bilinear sample with
// wrap, an effect's palette lookups, vertex colour, opacity, the palette
// alpha, an effect's falloff, the colour clamped to 0..1, alpha test,
// zero-alpha skip. Blending is set on the pipeline, not here.
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
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: SurfaceParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var diffuse: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var diffuse_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var palette: texture_2d<f32>;

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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
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
