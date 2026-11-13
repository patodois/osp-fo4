//! Geometry bounds in model coordinates, using authored NiAVObject transforms.
use fo4_core::{Budget, Error, Limits, Result};
#[cfg(test)]
use fo4_nif::Field;
use fo4_nif::{Data, Nif, Value};
use serde::Serialize;
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}
use crate::transform::Transform;
pub fn model(n: &Nif<'_>, file: &str, limits: &Limits, budget: &Budget) -> Result<Bounds> {
    struct Walk<'a, 'n> {
        n: &'a Nif<'n>,
        file: &'a str,
        limits: &'a Limits,
        budget: &'a Budget,
        active: Vec<usize>,
        visits: u64,
        bounds: Option<Bounds>,
    }
    impl Walk<'_, '_> {
        fn visit(&mut self, index: usize, parent: Transform, depth: u64) -> Result<()> {
            fo4_core::limit(depth, self.limits.max_depth, self.file, 0)?;
            self.visits = fo4_core::add(self.visits, 1, self.file, 0)?;
            fo4_core::limit(self.visits, self.limits.max_entries, self.file, 0)?;
            if self.active.contains(&index) {
                return Err(Error::new(
                    "BoundsNodeCycle",
                    self.file,
                    0,
                    "acyclic model nodes",
                    index,
                ));
            }
            let block = self.n.blocks.get(index).ok_or_else(|| {
                Error::new("BoundsBlockIndex", self.file, 0, "existing block", index)
            })?;
            let ty = &self.n.block_types[block.type_index as usize];
            if matches!(
                crate::model_policy::block_class(ty),
                fo4_plugin::still::Class::Outside | fo4_plugin::still::Class::State
            ) {
                return Ok(());
            }
            let data = block.decoded.as_ref().ok_or_else(|| {
                Error::new(
                    "BoundsOpaqueBlock",
                    self.file,
                    block.data.start as u64,
                    "decoded node or geometry",
                    ty,
                )
            })?;
            self.budget
                .reserve(&mut self.active, 1, self.file, block.data.start as u64)?;
            self.active.push(index);
            let fields = crate::model_policy::fields(data);
            let has_transform =
                matches!(data, Data::Node { .. }) || fields.iter().any(|f| f.name == "Translation");
            let transform = if has_transform {
                parent.compose(Transform::from_fields(fields, self.file)?)
            } else {
                parent
            };
            match data {
                Data::Node { children, .. } => {
                    for child in children.iter().filter(|i| **i >= 0) {
                        self.visit(*child as usize, transform, depth + 1)?;
                    }
                }
                Data::Mesh(mesh) => {
                    let vertices = mesh.vertices.as_ref().ok_or_else(|| {
                        Error::new(
                            "BoundsMissingVertices",
                            self.file,
                            block.data.start as u64,
                            "decoded positions",
                            mesh.declared_vertices,
                        )
                    })?;
                    if mesh.dynamic_vertices.is_some() {
                        return Err(Error::new(
                            "BoundsDynamicGeometry",
                            self.file,
                            block.data.start as u64,
                            "static vertices",
                            "dynamic vertex stream",
                        ));
                    }
                    for vertex in vertices {
                        let point = vertex.position.ok_or_else(|| {
                            Error::new(
                                "BoundsMissingPosition",
                                self.file,
                                block.data.start as u64,
                                "position attribute",
                                "absent",
                            )
                        })?;
                        let point = transform.apply(point.map(f64::from));
                        if point.iter().any(|v| !v.is_finite()) {
                            return Err(Error::new(
                                "NonFiniteBounds",
                                self.file,
                                block.data.start as u64,
                                "finite transformed point",
                                format!("{point:?}"),
                            ));
                        }
                        match &mut self.bounds {
                            None => {
                                self.bounds = Some(Bounds {
                                    min: point,
                                    max: point,
                                })
                            }
                            Some(bounds) => {
                                for (i, value) in point.iter().enumerate() {
                                    bounds.min[i] = bounds.min[i].min(*value);
                                    bounds.max[i] = bounds.max[i].max(*value);
                                }
                            }
                        }
                    }
                }
                Data::Fields(fields) => {
                    for f in fields.iter().filter(|f| f.name == "Data") {
                        if let Value::Ref(target) = f.value {
                            if target >= 0 {
                                self.visit(target as usize, transform, depth + 1)?;
                            }
                        }
                    }
                }
            }
            self.active.pop();
            Ok(())
        }
    }
    let scratch = budget.child();
    let mut walk = Walk {
        n,
        file,
        limits,
        budget: &scratch,
        active: Vec::new(),
        visits: 0,
        bounds: None,
    };
    for root in n.roots.iter().filter(|r| **r >= 0) {
        walk.visit(*root as usize, Transform::IDENTITY, 0)?;
    }
    walk.bounds.ok_or_else(|| {
        Error::new(
            "BoundsMissingGeometry",
            file,
            0,
            "reachable decoded vertices",
            "none",
        )
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn column_major_rotation_uniform_scale_and_parent_composition() {
        let mut fields = Vec::new();
        for (name, values) in [
            ("Translation", vec![10f32, 20., 30.]),
            ("Rotation", vec![0., 1., 0., -1., 0., 0., 0., 0., 1.]),
            ("Scale", vec![2.]),
        ] {
            fields.extend(values.into_iter().map(|v| Field {
                name,
                offset: 0,
                value: Value::FloatBits(v.to_bits()),
            }));
        }
        let transform = Transform::from_fields(&fields, "synthetic").unwrap();
        assert_eq!(transform.apply([1., 2., 3.]), [6., 22., 36.]);
        let parent = Transform {
            translation: [100., 0., 0.],
            ..Transform::IDENTITY
        };
        assert_eq!(
            parent.compose(transform).apply([1., 2., 3.]),
            [106., 22., 36.]
        );
        fields.pop();
        assert_eq!(
            Transform::from_fields(&fields, "synthetic")
                .err()
                .unwrap()
                .code,
            "BoundsTransformCount"
        );
    }
    #[test]
    fn model_bounds_include_child_transforms_and_refuse_missing_positions() {
        fn av(translation: [f32; 3]) -> Vec<Field> {
            let mut fields = Vec::new();
            for (name, values) in [
                ("Translation", translation.to_vec()),
                ("Rotation", vec![1., 0., 0., 0., 1., 0., 0., 0., 1.]),
                ("Scale", vec![1.]),
            ] {
                fields.extend(values.into_iter().map(|v| Field {
                    name,
                    offset: 0,
                    value: Value::FloatBits(v.to_bits()),
                }));
            }
            fields
        }
        let mesh = fo4_nif::Mesh {
            non_finite: Default::default(),
            fields: av([3., 0., 0.]),
            vertex_desc: Some(0),
            declared_vertices: 2,
            declared_triangles: 0,
            vertices: Some(vec![
                fo4_nif::Vertex {
                    position: Some([-1., -2., -3.]),
                    ..Default::default()
                },
                fo4_nif::Vertex {
                    position: Some([1., 2., 3.]),
                    ..Default::default()
                },
            ]),
            triangles: Some(Vec::new()),
            dynamic_vertices: None,
            raw_vertex_aux: Vec::new(),
        };
        let mut n = Nif {
            version: 0x14020007,
            user_version: 12,
            bs_version: 130,
            exports: Vec::new(),
            block_types: vec!["NiNode".into(), "BSTriShape".into()],
            strings: Vec::new(),
            max_string_length: 0,
            groups: Vec::new(),
            roots: vec![0],
            blocks: vec![
                fo4_nif::Block {
                    type_index: 0,
                    data: 0..0,
                    decoded: Some(Data::Node {
                        fields: av([10., 0., 0.]),
                        children: vec![1],
                    }),
                    raw_tail: 0..0,
                    diagnostics: Vec::new(),
                },
                fo4_nif::Block {
                    type_index: 1,
                    data: 0..0,
                    decoded: Some(Data::Mesh(mesh)),
                    raw_tail: 0..0,
                    diagnostics: Vec::new(),
                },
            ],
            bytes: &[],
        };
        let l = Limits::default();
        assert_eq!(
            model(&n, "synthetic", &l, &Budget::new(4096)).unwrap(),
            Bounds {
                min: [12., -2., -3.],
                max: [14., 2., 3.]
            }
        );
        assert!(model(&n, "synthetic", &l, &Budget::new(0)).is_err());
        assert!(model(
            &n,
            "synthetic",
            &Limits {
                max_depth: 0,
                ..l.clone()
            },
            &Budget::new(4096)
        )
        .is_err());
        if let Some(Data::Mesh(mesh)) = &mut n.blocks[1].decoded {
            mesh.vertices.as_mut().unwrap()[0].position = None;
        }
        assert_eq!(
            model(&n, "synthetic", &l, &Budget::new(4096))
                .unwrap_err()
                .code,
            "BoundsMissingPosition"
        );
    }
}
