//! Lossless JSON representation of IEEE values, plus allocation-free counts.
use serde::{
    ser::{SerializeMap, SerializeSeq},
    Serialize, Serializer,
};
pub trait Values {
    fn non_finite_count(&self) -> u64;
    fn serialize_ieee<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
}
struct Encoded<'a, T>(&'a T);
impl<T: Values> Serialize for Encoded<'_, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize_ieee(s)
    }
}
pub fn serialize<T: Values, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
    v.serialize_ieee(s)
}
pub fn count<T: Values>(v: &T) -> u64 {
    v.non_finite_count()
}
impl Values for f32 {
    fn non_finite_count(&self) -> u64 {
        u64::from(!self.is_finite())
    }
    fn serialize_ieee<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.is_finite() {
            s.serialize_f32(*self)
        } else {
            let mut map = s.serialize_map(Some(1))?;
            map.serialize_entry("ieee754_bits", &self.to_bits())?;
            map.end()
        }
    }
}
impl<T: Values, const N: usize> Values for [T; N] {
    fn non_finite_count(&self) -> u64 {
        self.iter().map(Values::non_finite_count).sum()
    }
    fn serialize_ieee<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(N))?;
        for v in self {
            seq.serialize_element(&Encoded(v))?;
        }
        seq.end()
    }
}
impl<T: Values> Values for Vec<T> {
    fn non_finite_count(&self) -> u64 {
        self.iter().map(Values::non_finite_count).sum()
    }
    fn serialize_ieee<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.len()))?;
        for v in self {
            seq.serialize_element(&Encoded(v))?;
        }
        seq.end()
    }
}
impl<T: Values> Values for Option<T> {
    fn non_finite_count(&self) -> u64 {
        self.as_ref().map_or(0, Values::non_finite_count)
    }
    fn serialize_ieee<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Some(v) => s.serialize_some(&Encoded(v)),
            None => s.serialize_none(),
        }
    }
}
