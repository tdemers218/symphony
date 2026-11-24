/// 2D affine transform, matrix form:
/// ```text
/// | a c tx |
/// | b d ty |
/// ```
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub fn translate(x: f32, y: f32) -> Self {
        Affine {
            tx: x,
            ty: y,
            ..Self::IDENTITY
        }
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Affine {
            a: sx,
            d: sy,
            ..Self::IDENTITY
        }
    }

    /// Counter-clockwise rotation in degrees.
    pub fn rotate_deg(deg: f32) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Affine {
            a: c,
            b: s,
            c: -s,
            d: c,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// `self.compose(rhs)` is `self ∘ rhs`: apply `rhs` first, then `self`.
    pub fn compose(self, r: Affine) -> Affine {
        let s = self;
        Affine {
            a: s.a * r.a + s.c * r.b,
            b: s.b * r.a + s.d * r.b,
            c: s.a * r.c + s.c * r.d,
            d: s.b * r.c + s.d * r.d,
            tx: s.a * r.tx + s.c * r.ty + s.tx,
            ty: s.b * r.tx + s.d * r.ty + s.ty,
        }
    }

    /// Inverse transform, or `None` if it squashes everything to a line.
    pub fn inverse(self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine {
            a,
            b,
            c,
            d,
            tx: -(a * self.tx + c * self.ty),
            ty: -(b * self.tx + d * self.ty),
        })
    }

    pub fn apply(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_then_translate() {
        let m = Affine::translate(10.0, 0.0).compose(Affine::rotate_deg(90.0));
        let (x, y) = m.apply(1.0, 0.0);
        assert!((x - 10.0).abs() < 1e-5 && (y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn inverse_round_trips() {
        let m = Affine::translate(5.0, -3.0)
            .compose(Affine::rotate_deg(30.0))
            .compose(Affine::scale(2.0, 0.5));
        let (x, y) = m
            .inverse()
            .unwrap()
            .apply(m.apply(7.0, 4.0).0, m.apply(7.0, 4.0).1);
        assert!((x - 7.0).abs() < 1e-4 && (y - 4.0).abs() < 1e-4);
        assert!(Affine::scale(0.0, 1.0).inverse().is_none());
    }
}
