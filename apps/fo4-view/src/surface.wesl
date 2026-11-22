// Unlit surface of the GPU viewer. Mirrors the per-sample steps of the CPU
// reference rasterizer (crates/fo4-render/src/raster.rs, `Raster::triangle`):
// UV transform, mip level from the screen footprint, bilinear sample with
// wrap, vertex colour, opacity, alpha test, zero-alpha skip. Blending is set
// on the pipeline, not here.
import bevy_pbr::render::forward_io::VertexOutput;
@if(TARGET_SRGB) import bevy_render::color_operations::srgb_to_linear;

struct SurfaceParams {
    uv_scale: vec2<f32>,
    uv_offset: vec2<f32>,
    test_ref: f32,
    opacity: f32,
    // NiAlphaProperty function 0..7, 8 = CPU reference threshold (>=), 9 = no test.
    test_function: u32,
    skip_zero_alpha: u32,
    // Draw::instance, written as four bytes by the instance-tag pass.
    tag: u32,
    // 1: the texture's alpha is replaced by 1 (stored BC1 blocks declared
    // without alpha, terrain textures), as the CPU's decoded texels hold it.
    opaque_texture: u32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: SurfaceParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var diffuse: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var diffuse_sampler: sampler;

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
    @if(VERTEX_COLORS)
    color = color * in.color;
    color.a = color.a * params.opacity;
    if !passes(color.a) {
        discard;
    }
    if params.skip_zero_alpha == 1u && color.a == 0.0 {
        discard;
    }
    @if(INSTANCE_TAG) {
        // Byte k of the tag in channel k: an RGBA8 unorm target stores the
        // tag's little-endian bytes, the layout of `Frame::instance_bytes`.
        return unpack4x8unorm(params.tag);
    } @else {
        // The CPU writes its working values as stored bytes. On an sRGB
        // target the hardware encodes on store, so decode first to store the
        // same bytes.
        @if(TARGET_SRGB)
        color = vec4<f32>(srgb_to_linear(color.rgb), color.a);
        return color;
    }
}
