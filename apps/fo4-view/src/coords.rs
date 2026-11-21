//! The one place where game coordinates become Bevy coordinates.
//!
//! The game world is right-handed and Z-up: `fo4_scene::camera::look_at`
//! builds `right = forward x (0, 0, 1)`, so facing +X the right-hand side is
//! -Y. Bevy is right-handed and Y-up, and a camera looks down its local -Z
//! with +X to the right and +Y up.
//!
//! The conversion is the rotation `(x, y, z) -> (x, z, -y)` (determinant +1),
//! so handedness is preserved and nothing is mirrored. Meshes keep their
//! game-local vertices; each instance's matrix is `GAME_TO_BEVY * world`.
use bevy::math::{Affine3A, Mat3, Mat4, Quat, Vec3, Vec3A, Vec4};
use fo4_scene::{camera::Camera, transform::Transform as GameTransform};

/// Rows of the game-to-Bevy rotation, applied as `bevy = GAME_TO_BEVY * game`.
pub const GAME_TO_BEVY: [[f64; 3]; 3] = [[1., 0., 0.], [0., 0., 1.], [0., -1., 0.]];

pub fn game_to_bevy(p: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|j| GAME_TO_BEVY[i][j] * p[j]).sum())
}
fn vec3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// Matrix taking a draw's game-local vertex to Bevy world space, exactly
/// `game_to_bevy(world.apply(p))` (`Transform::apply` uses `R^T`).
pub fn instance_affine(world: &GameTransform) -> Affine3A {
    let column = |j: usize| -> Vec3 {
        // Column j of the game linear map is s * R[j]; then rotate into Bevy.
        vec3(game_to_bevy(world.rotation[j].map(|v| v * world.scale)))
    };
    Affine3A::from_mat3_translation(
        Mat3::from_cols(column(0), column(1), column(2)),
        vec3(game_to_bevy(world.translation)),
    )
}

/// Bevy translation, rotation and scale of a draw, or `None` when the draw's
/// matrix is not a rotation times an axis scale (a shear, which Bevy's
/// `Transform` cannot hold; the caller reports it, nothing is guessed).
/// A mirrored matrix comes back with a negative scale component.
pub fn instance_transform(world: &GameTransform) -> Option<(Vec3, Quat, Vec3)> {
    let affine = instance_affine(world);
    let (scale, rotation, translation) = affine.to_scale_rotation_translation();
    let rebuilt = Affine3A::from_scale_rotation_translation(scale, rotation, translation);
    let tolerance = 1e-4
        * affine
            .matrix3
            .abs()
            .to_cols_array()
            .iter()
            .fold(1f32, |a, b| a.max(*b));
    let close = affine
        .matrix3
        .to_cols_array()
        .iter()
        .zip(rebuilt.matrix3.to_cols_array())
        .all(|(a, b)| (a - b).abs() <= tolerance);
    close.then_some((translation, rotation, scale))
}

/// Bevy camera pose for a CPU reference camera: translation and rotation
/// whose local +X, +Y, -Z are the camera's right, up and forward.
pub fn camera_pose(camera: &Camera) -> Result<(Vec3, Quat), String> {
    if camera.scale != 1. || camera.viewport != [0., 1., 1., 0.] {
        return Err(format!(
            "UnsupportedViewerCamera: scale {} viewport {:?}; the viewer needs scale 1 and the full frame",
            camera.scale, camera.viewport
        ));
    }
    let [forward, up, right] = camera.rotation.map(|row| vec3(game_to_bevy(row)));
    let basis = Mat3::from_cols(right, up, -forward);
    Ok((
        vec3(game_to_bevy(camera.translation)),
        Quat::from_mat3(&basis),
    ))
}

/// Clip-from-view matrix that reproduces `Camera::project_coordinates` for
/// the frustum `[left, right, top, bottom, near, far]` (slopes and forward
/// distances), in Bevy's reverse-Z convention: depth 1 at `near`, 0 at `far`.
/// View space is Bevy's: right = +X, up = +Y, forward = -Z.
pub fn clip_from_view(frustum: [f64; 6]) -> Mat4 {
    let [l, r, t, b, n, f] = frustum;
    Mat4::from_cols(
        Vec4::new((2. / (r - l)) as f32, 0., 0., 0.),
        Vec4::new(0., (2. / (t - b)) as f32, 0., 0.),
        Vec4::new(
            ((r + l) / (r - l)) as f32,
            ((t + b) / (t - b)) as f32,
            (n / (f - n)) as f32,
            -1.,
        ),
        Vec4::new(0., 0., (n * f / (f - n)) as f32, 0.),
    )
}

/// The part of `frustum` seen by a sub-rectangle of the full frame, in pixels
/// from the top-left corner.
pub fn sub_frustum(
    frustum: [f64; 6],
    full: [f64; 2],
    offset: [f64; 2],
    size: [f64; 2],
) -> [f64; 6] {
    let [l, r, t, b, n, f] = frustum;
    let x = |p: f64| l + (r - l) * p / full[0];
    let y = |p: f64| t - (t - b) * p / full[1];
    [
        x(offset[0]),
        x(offset[0] + size[0]),
        y(offset[1]),
        y(offset[1] + size[1]),
        n,
        f,
    ]
}

/// Frustum corners at view-space depths `z_near` and `z_far` (negative in
/// front of the camera), in the order Bevy's `CameraProjection` documents:
/// bottom right, top right, top left, bottom left; near then far.
pub fn frustum_corners(frustum: [f64; 6], z_near: f32, z_far: f32) -> [Vec3A; 8] {
    let [l, r, t, b, _, _] = frustum.map(|v| v as f32);
    let ring = |z: f32| {
        let d = z.abs();
        [
            Vec3A::new(r * d, b * d, z),
            Vec3A::new(r * d, t * d, z),
            Vec3A::new(l * d, t * d, z),
            Vec3A::new(l * d, b * d, z),
        ]
    };
    let [a, b2, c, d] = ring(z_near);
    let [e, f, g, h] = ring(z_far);
    [a, b2, c, d, e, f, g, h]
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_scene::camera::{look_at, reference_frustum};

    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    #[test]
    fn conversion_is_a_proper_rotation_with_game_up_to_bevy_up() {
        let m = GAME_TO_BEVY;
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        assert_eq!(det, 1.);
        assert_eq!(game_to_bevy([0., 0., 1.]), [0., 1., 0.]);
        let (a, b) = ([0.3, -1.2, 2.5], [-4., 0.5, 1.5]);
        assert_eq!(
            game_to_bevy(cross(a, b)),
            cross(game_to_bevy(a), game_to_bevy(b))
        );
    }

    /// The GPU path (instance matrix, camera pose, projection, viewport) lands
    /// every point on the same pixel as the CPU reference projection. A
    /// mirrored conversion would move points to the other side of the frame.
    #[test]
    fn gpu_projection_matches_cpu_reference_pixels() {
        let size = [1280u32, 720];
        let frustum = reference_frustum(size);
        let camera = look_at([0., 153.6, 104.49], [70.7, 224.3, 104.49], frustum, size).unwrap();
        let world = GameTransform {
            rotation: fo4_scene::transform::euler_zyx([0.2, -0.4, 1.1], false)
                .map(|r| r.map(f64::from)),
            translation: [40., 200., 90.],
            scale: 1.5,
        };
        let (eye, rotation) = camera_pose(&camera).unwrap();
        let view_from_world = Affine3A::from_rotation_translation(rotation, eye).inverse();
        let clip = clip_from_view(frustum);
        let affine = instance_affine(&world);
        let mut seen = 0;
        for local in [
            [0., 0., 0.],
            [10., -5., 3.],
            [-20., 8., 12.],
            [3., 30., -9.],
        ] {
            let cpu = camera.project(world.apply(local)).unwrap();
            let p = clip
                * view_from_world
                    .transform_point3(affine.transform_point3(vec3(local)))
                    .extend(1.);
            let ndc = [p.x / p.w, p.y / p.w];
            let gpu = [
                (ndc[0] + 1.) * 0.5 * size[0] as f32,
                (1. - ndc[1]) * 0.5 * size[1] as f32,
            ];
            let Some(cpu) = cpu else {
                assert!(p.w <= 0.);
                continue;
            };
            seen += 1;
            for k in 0..2 {
                assert!(
                    (cpu.pixel[k] - gpu[k] as f64).abs() < 1e-2,
                    "{local:?}: cpu {:?} gpu {gpu:?}",
                    cpu.pixel
                );
            }
            // Reverse-Z depth stays inside [0, 1] between near and far.
            let depth = p.z / p.w;
            assert!((0. ..=1.).contains(&depth));
            assert!((p.w as f64 - cpu.forward).abs() < 1e-3);
        }
        assert_eq!(seen, 4);
    }

    #[test]
    fn reverse_z_puts_near_at_one_and_far_at_zero() {
        let clip = clip_from_view([-1., 1., 0.5, -0.5, 15., 12000.]);
        let near = clip * Vec4::new(0., 0., -15., 1.);
        let far = clip * Vec4::new(0., 0., -12000., 1.);
        assert!((near.z / near.w - 1.).abs() < 1e-6);
        assert!((far.z / far.w).abs() < 1e-6);
    }

    #[test]
    fn instance_transform_accepts_similarities_and_refuses_shear() {
        let world = GameTransform {
            rotation: fo4_scene::transform::euler_zyx([0.7, 0.1, -2.0], false)
                .map(|r| r.map(f64::from)),
            translation: [1., 2., 3.],
            scale: 0.75,
        };
        let (t, r, s) = instance_transform(&world).unwrap();
        let rebuilt = Affine3A::from_scale_rotation_translation(s, r, t);
        let p = [4., -5., 6.];
        let want = vec3(game_to_bevy(world.apply(p)));
        assert!(rebuilt.transform_point3(vec3(p)).abs_diff_eq(want, 1e-4));
        let shear = GameTransform {
            rotation: [[1., 0.5, 0.], [0., 1., 0.], [0., 0., 1.]],
            translation: [0.; 3],
            scale: 1.,
        };
        assert!(instance_transform(&shear).is_none());
    }

    #[test]
    fn sub_frustum_of_the_whole_frame_is_the_frame() {
        let f = [-1., 1., 0.5, -0.5, 15., 12000.];
        assert_eq!(sub_frustum(f, [100., 50.], [0., 0.], [100., 50.]), f);
        let right_half = sub_frustum(f, [100., 50.], [50., 0.], [50., 50.]);
        assert_eq!(right_half, [0., 1., 0.5, -0.5, 15., 12000.]);
    }
}
