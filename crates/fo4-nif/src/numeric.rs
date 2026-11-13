use crate::{Data, Field, Mesh, Nif, Value};
use fo4_core::ieee::count;
use serde::Serialize;
#[derive(Debug, Default, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct NonFiniteCounts {
    pub position: u64,
    pub uv: u64,
    pub normal: u64,
    pub tangent: u64,
    pub bitangent: u64,
    pub color: u64,
    pub eye: u64,
    pub dynamic_position: u64,
    pub fields: u64,
}
impl NonFiniteCounts {
    pub fn attributes(&self) -> [(&'static str, u64); 9] {
        [
            ("position", self.position),
            ("uv", self.uv),
            ("normal", self.normal),
            ("tangent", self.tangent),
            ("bitangent", self.bitangent),
            ("color", self.color),
            ("eye", self.eye),
            ("dynamic_position", self.dynamic_position),
            ("fields", self.fields),
        ]
    }
    pub fn total(&self) -> u64 {
        self.attributes().iter().map(|(_, n)| n).sum()
    }
    pub fn merge(&mut self, v: &Self) {
        self.position += v.position;
        self.uv += v.uv;
        self.normal += v.normal;
        self.tangent += v.tangent;
        self.bitangent += v.bitangent;
        self.color += v.color;
        self.eye += v.eye;
        self.dynamic_position += v.dynamic_position;
        self.fields += v.fields;
    }
    pub(crate) fn from_fields(fields: &[Field]) -> Self {
        Self{fields:fields.iter().filter(|f|matches!(f.value,Value::FloatBits(bits) if !f32::from_bits(bits).is_finite())).count() as u64,..Self::default()}
    }
    pub(crate) fn from_mesh(m: &Mesh) -> Self {
        let mut n = Self::from_fields(&m.fields);
        if let Some(vertices) = &m.vertices {
            for v in vertices {
                n.position += count(&v.position);
                n.uv += count(&v.uv);
                n.normal += count(&v.normal);
                n.tangent += count(&v.tangent);
                n.bitangent += count(&v.bitangent);
                n.color += count(&v.color_float);
                n.eye += count(&v.eye);
            }
        }
        n.dynamic_position = count(&m.dynamic_vertices);
        n
    }
}
impl Nif<'_> {
    pub fn non_finite(&self) -> NonFiniteCounts {
        let mut n = NonFiniteCounts::default();
        for b in &self.blocks {
            if let Some(d) = &b.decoded {
                n.merge(&match d {
                    Data::Mesh(m) => m.non_finite,
                    Data::Node { fields, .. } | Data::Fields(fields) => {
                        NonFiniteCounts::from_fields(fields)
                    }
                });
            }
        }
        n
    }
}
