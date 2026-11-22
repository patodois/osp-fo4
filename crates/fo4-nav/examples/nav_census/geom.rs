//! Small 2D geometry for the grid census.

/// Whether a triangle's top-down footprint and the rectangle
/// `[x0, x1] x [y0, y1]` share a point, by separating axes, with `eps`
/// slack (positive makes touching count).
pub fn triangle_meets_rect(
    t: &[[f32; 3]; 3],
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    eps: f64,
) -> bool {
    let p: Vec<[f64; 2]> = t
        .iter()
        .map(|v| [f64::from(v[0]), f64::from(v[1])])
        .collect();
    let (tx0, tx1) = (
        p.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min),
        p.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max),
    );
    let (ty0, ty1) = (
        p.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min),
        p.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max),
    );
    if tx1 < x0 - eps || tx0 > x1 + eps || ty1 < y0 - eps || ty0 > y1 + eps {
        return false;
    }
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    // Orientation of the triangle; a degenerate one meets by its box.
    let area =
        (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
    if area.abs() < 1e-12 {
        return true;
    }
    let s = area.signum();
    for (i, a) in p.iter().enumerate() {
        let a = *a;
        let b = p[(i + 1) % 3];
        let (nx, ny) = (b[1] - a[1], a[0] - b[0]);
        let len = (nx * nx + ny * ny).sqrt();
        // (nx, ny) points out of a counter-clockwise triangle; s flips it
        // for a clockwise one.
        let outside = corners.iter().all(|c| {
            let d = s * (nx * (c[0] - a[0]) + ny * (c[1] - a[1])) / len;
            d > eps
        });
        if outside {
            return false;
        }
    }
    true
}

/// Whether a triangle's top-down bounding box meets the rectangle.
pub fn box_meets_rect(t: &[[f32; 3]; 3], x0: f64, x1: f64, y0: f64, y1: f64, eps: f64) -> bool {
    let xs = t.map(|v| f64::from(v[0]));
    let ys = t.map(|v| f64::from(v[1]));
    let (tx0, tx1) = (
        xs.iter().copied().fold(f64::INFINITY, f64::min),
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    let (ty0, ty1) = (
        ys.iter().copied().fold(f64::INFINITY, f64::min),
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    !(tx1 < x0 - eps || tx0 > x1 + eps || ty1 < y0 - eps || ty0 > y1 + eps)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separating_axes() {
        let t = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]];
        assert!(triangle_meets_rect(&t, 1.0, 2.0, 1.0, 2.0, 0.0));
        assert!(!triangle_meets_rect(&t, 8.0, 9.0, 8.0, 9.0, 0.0));
        assert!(box_meets_rect(&t, 8.0, 9.0, 8.0, 9.0, 0.0));
        let r = [[0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [10.0, 0.0, 0.0]];
        assert!(!triangle_meets_rect(&r, 8.0, 9.0, 8.0, 9.0, 0.0));
        assert!(triangle_meets_rect(&r, 5.0, 9.0, 5.0, 9.0, 0.0));
    }
}
