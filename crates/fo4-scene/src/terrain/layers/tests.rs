use super::*;
use fo4_plugin::land::{AlphaHeader, AlphaLayer, AlphaPoint, BaseLayer, PointGrid};
use fo4_plugin::typed::FormRef;

fn key(id: u32) -> FormKey {
    FormKey {
        plugin: "synthetic.esm".into(),
        object_id: id,
    }
}
fn form(id: u32) -> Option<FormRef> {
    (id != 0).then(|| FormRef {
        raw: id,
        key: Some(key(id)),
        subrecord: 0,
    })
}
fn empty() -> Land {
    Land {
        data: None,
        heights: None,
        normals: None,
        colors: None,
        base_layers: Vec::new(),
        alpha_layers: Vec::new(),
        textures: Vec::new(),
        uninterpreted: Vec::new(),
        non_finite_values: 0,
    }
}
fn base(quadrant: u8, id: u32) -> BaseLayer {
    BaseLayer {
        subrecord: 0,
        texture: form(id),
        quadrant,
        raw: [0; 3],
    }
}
fn layer(quadrant: u8, number: u16, id: u32, points: &[(u16, f32)]) -> AlphaLayer {
    AlphaLayer {
        header: AlphaHeader {
            subrecord: 0,
            texture: form(id),
            quadrant,
            unknown: 0,
            layer: number,
        },
        point_subrecords: vec![0],
        points: points
            .iter()
            .map(|(position, opacity)| AlphaPoint {
                position: *position,
                unknown: [0; 2],
                opacity: *opacity,
            })
            .collect(),
    }
}
fn build(land: &Land) -> Layers {
    from_land(key(1), land, State::Blocked, &Budget::new(1 << 20)).unwrap()
}
fn weight(q: &Quadrant, s: &Source, vertex: usize) -> f32 {
    q.textures
        .iter()
        .find(|t| t.source == *s)
        .map_or(0.0, |t| t.weights[vertex])
}

#[test]
fn quadrants_positions_and_texture_coordinates_follow_the_candidates() {
    assert_eq!(
        (0..5).map(quadrant_origin).collect::<Vec<_>>(),
        [
            Some([0, 0]),
            Some([0, 16]),
            Some([16, 0]),
            Some([16, 16]),
            None
        ]
    );
    assert_eq!(position_vertex(0), Some([0, 0]));
    assert_eq!(position_vertex(16), Some([0, 16]));
    assert_eq!(position_vertex(17), Some([1, 0]));
    assert_eq!(position_vertex(288), Some([16, 16]));
    assert_eq!(position_vertex(289), None);
    // 128-unit vertex steps, one repeat per 256 units: half a repeat per
    // step; v grows south. A cell holds 16 whole repeats.
    assert_eq!(texture_uv(0, 0), [0.0, 0.0]);
    assert_eq!(texture_uv(2, 4), [2.0, -1.0]);
    assert_eq!(texture_uv(32, 32), [16.0, -16.0]);
    let quads: Vec<_> = quadrant_quads(&build(&empty()).quadrants[3]).collect();
    assert_eq!(quads.len(), 256);
    assert_eq!((quads[0], quads[255]), ([16, 16], [31, 31]));
    assert_eq!(quad_index(31, 31), 32 * 32 - 1);
}

#[test]
fn opacities_are_weights_and_the_base_takes_the_rest() {
    let mut land = empty();
    land.base_layers.push(base(0, 0xa));
    // Stored out of layer order; the same texture twice merges.
    land.alpha_layers
        .push(layer(0, 1, 0xb, &[(0, 0.25), (5, 1.0), (288, 0.5)]));
    land.alpha_layers
        .push(layer(0, 0, 0xc, &[(0, 0.5), (5, 0.25)]));
    land.alpha_layers.push(layer(0, 2, 0xa, &[(288, 0.25)]));
    let l = build(&land);
    let q = &l.quadrants[0];
    assert_eq!((q.refusal, q.layers), (None, 3));
    let (a, b, c) = (
        Source::Ltex(key(0xa)),
        Source::Ltex(key(0xb)),
        Source::Ltex(key(0xc)),
    );
    // Base first, then layers by ascending number; A appears once.
    let order: Vec<_> = q.textures.iter().map(|t| t.source.clone()).collect();
    assert_eq!(order, [a.clone(), c.clone(), b.clone()]);
    assert_eq!(
        (weight(q, &a, 0), weight(q, &b, 0), weight(q, &c, 0)),
        (0.25, 0.25, 0.5)
    );
    // Sum above one: no base, the layers as stored.
    assert_eq!(
        (weight(q, &a, 5), weight(q, &b, 5), weight(q, &c, 5)),
        (0.0, 1.0, 0.25)
    );
    assert_eq!(q.over_unity_vertices, 1);
    // A as base (0.25) and as layer 2 (0.25), B 0.5.
    assert_eq!((weight(q, &a, 288), weight(q, &b, 288)), (0.5, 0.5));
    // Vertices no entry names: the base alone.
    assert_eq!(weight(q, &a, 100), 1.0);
    for v in [0, 100, 288] {
        let sum: f32 = q.textures.iter().map(|t| t.weights[v]).sum();
        assert_eq!(sum, 1.0);
    }
}

#[test]
fn a_missing_base_and_a_null_layer_are_one_default_texture() {
    let mut land = empty();
    land.alpha_layers.push(layer(1, 0, 0, &[(3, 0.25)]));
    land.alpha_layers
        .push(layer(1, 1, 0xd, &[(3, 0.5), (4, 1.0)]));
    let l = build(&land);
    let q = &l.quadrants[1];
    assert_eq!(q.base, Some(Source::Default));
    assert_eq!(q.textures.len(), 2);
    let d = Source::Default;
    // Residual 0.25 plus the null layer's 0.25.
    assert_eq!(weight(q, &d, 3), 0.5);
    assert_eq!(weight(q, &d, 4), 0.0);
    assert_eq!(weight(q, &d, 0), 1.0);
    // Quadrants with no subrecord at all are the default everywhere.
    for q in &l.quadrants[2..] {
        assert_eq!((q.refusal, q.layers, q.textures.len()), (None, 0, 1));
        assert!(q.textures[0].weights.iter().all(|w| *w == 1.0));
    }
}

#[test]
fn malformed_layers_refuse_their_quadrant_only() {
    let mut land = empty();
    land.base_layers.push(base(0, 0xa));
    land.base_layers.push(base(1, 0xa));
    land.base_layers.push(base(1, 0xb));
    land.alpha_layers.push(layer(2, 0, 0xc, &[(289, 0.5)]));
    land.alpha_layers
        .push(layer(3, 0, 0xc, &[(7, 0.5), (7, 0.25)]));
    let l = build(&land);
    let refusals: Vec<_> = l.quadrants.iter().map(|q| q.refusal).collect();
    assert_eq!(
        refusals,
        [
            None,
            Some("MultipleBaseLayers"),
            Some("LayerPositionOutsideQuadrant"),
            Some("DuplicateLayerPosition"),
        ]
    );
    assert!(l.quadrants[1..].iter().all(|q| q.textures.is_empty()));
    for bad in [f32::NAN, -0.5, 1.5] {
        let mut land = empty();
        land.alpha_layers.push(layer(0, 0, 0xc, &[(0, bad)]));
        assert_eq!(
            build(&land).quadrants[0].refusal,
            Some("LayerOpacityOutsideUnitRange")
        );
    }
    // A FormID that did not resolve is not the default.
    let mut land = empty();
    land.base_layers.push(BaseLayer {
        texture: Some(FormRef {
            raw: 0x10,
            key: None,
            subrecord: 0,
        }),
        ..base(0, 0)
    });
    assert_eq!(
        build(&land).quadrants[0].refusal,
        Some("UnresolvedBaseTexture")
    );
    assert!(from_land(key(1), &empty(), State::Blocked, &Budget::new(64)).is_err());
}

#[test]
fn vertex_colours_are_byte_fractions_in_grid_order() {
    let mut land = empty();
    assert!(build(&land).colors.is_none());
    let mut values = vec![[255u8; 3]; POINTS];
    values[34] = [0, 51, 102];
    land.colors = Some(PointGrid {
        subrecord: 0,
        values,
    });
    let l = build(&land);
    let c = l.colors.as_ref().unwrap();
    assert_eq!(c[34], [0.0, 0.2, 0.4]);
    assert_eq!(c[0], [1.0; 3]);
    // A grid of another length is not used.
    land.colors = Some(PointGrid {
        subrecord: 0,
        values: vec![[0; 3]; 3],
    });
    assert!(build(&land).colors.is_none());
    assert_eq!(l.rules.len(), RULES.len());
    assert!(l.rules.windows(2).all(|w| w[0] < w[1]));
}
