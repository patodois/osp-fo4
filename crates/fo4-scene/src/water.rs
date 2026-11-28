//! The water an exterior cell's record stores (`spec/play.md`, *Water*): the
//! water height (XCLW, a little-endian f32; xEdit's published name "Water
//! Height", layout in `spec/record-layouts.md#cell-xclw`) and the water
//! type (XCWT, a form id of a WATR record). What the game draws or does
//! with them is not read here: drawing a flat surface at that height over
//! the cell's square is the pending rule [`crate::hypotheses::CELL_WATER_PLANE`].
use fo4_core::{Budget, Result};
use fo4_plugin::typed::{Cell, Store};
use fo4_plugin::FormKey;
use serde::Serialize;

/// XCLW values at or above this, or not finite, are read as "no water":
/// the cells without water observed in the data store the largest finite
/// f32 (bytes `ff ff 7f 7f`).
pub const NO_WATER_FROM: f32 = 1.0e30;

/// One cell's stored water.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CellWater {
    pub cell: FormKey,
    /// The XCLW height (world units).
    pub height: f64,
    /// The XCWT water type, when the cell names one.
    pub water: Option<FormKey>,
}

/// The height an XCLW payload stores, `None` for the no-water sentinel
/// ([`NO_WATER_FROM`]), a non-finite value or a payload not four bytes long.
pub fn height_of(payload: &[u8]) -> Option<f64> {
    let bytes: [u8; 4] = payload.try_into().ok()?;
    let h = f32::from_le_bytes(bytes);
    (h.is_finite() && h.abs() < NO_WATER_FROM).then_some(f64::from(h))
}

/// The water `cell`'s winning record stores: `None` when it has no XCLW or
/// its XCLW holds the no-water sentinel. Nothing is assumed for a cell
/// without XCLW (the world's default water height is not read).
pub fn cell_water(store: &Store, cell: &FormKey, b: &Budget) -> Result<Option<CellWater>> {
    let Some(raw) = store.raw(cell, b)? else {
        return Ok(None);
    };
    let Some(height) = raw
        .subrecords
        .iter()
        .find(|s| &s.signature == b"XCLW")
        .and_then(|s| height_of(raw.data(s)))
    else {
        return Ok(None);
    };
    let water = store
        .get::<Cell>(cell, b)?
        .and_then(|c| c.data.water.and_then(|w| w.key));
    Ok(Some(CellWater {
        cell: cell.clone(),
        height,
        water,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_height_is_read_and_the_sentinel_is_no_water() {
        assert_eq!(height_of(&1234.5f32.to_le_bytes()), Some(1234.5));
        assert_eq!(height_of(&(-20.0f32).to_le_bytes()), Some(-20.0));
        assert_eq!(height_of(&[0xff, 0xff, 0x7f, 0x7f]), None);
        assert_eq!(height_of(&f32::NAN.to_le_bytes()), None);
        assert_eq!(height_of(&[0, 0, 0]), None);
    }
}
