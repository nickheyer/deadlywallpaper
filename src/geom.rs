use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn area(&self) -> i64 {
        self.w.max(0) as i64 * self.h.max(0) as i64
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn intersection(&self, o: &Rect) -> Option<Rect> {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        (r > x && b > y).then(|| Rect::new(x, y, r - x, b - y))
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.intersection(o).is_some()
    }

    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }

    pub fn bounds<'a>(rects: impl IntoIterator<Item = &'a Rect>) -> Rect {
        rects
            .into_iter()
            .fold(Rect::default(), |acc, r| acc.union(r))
    }

    /// Fraction of `self` covered by the union of `covers`, exact via scanline decomposition.
    pub fn coverage(&self, covers: &[Rect]) -> f64 {
        if self.is_empty() {
            return 0.0;
        }
        let clipped: Vec<Rect> = covers.iter().filter_map(|c| self.intersection(c)).collect();
        if clipped.is_empty() {
            return 0.0;
        }
        let mut xs: Vec<i32> = clipped.iter().flat_map(|r| [r.x, r.right()]).collect();
        xs.sort_unstable();
        xs.dedup();
        let mut covered: i64 = 0;
        for w in xs.windows(2) {
            let (x0, x1) = (w[0], w[1]);
            let mut spans: Vec<(i32, i32)> = clipped
                .iter()
                .filter(|r| r.x <= x0 && r.right() >= x1)
                .map(|r| (r.y, r.bottom()))
                .collect();
            spans.sort_unstable();
            let mut y_cov: i64 = 0;
            let mut cur: Option<(i32, i32)> = None;
            for (a, b) in spans {
                match cur {
                    Some((ca, cb)) if a <= cb => cur = Some((ca, cb.max(b))),
                    Some((ca, cb)) => {
                        y_cov += (cb - ca) as i64;
                        cur = Some((a, b));
                    }
                    None => cur = Some((a, b)),
                }
            }
            if let Some((ca, cb)) = cur {
                y_cov += (cb - ca) as i64;
            }
            covered += y_cov * (x1 - x0) as i64;
        }
        covered as f64 / self.area() as f64
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Size {
    pub w: i32,
    pub h: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_union_not_double_counted() {
        let d = Rect::new(0, 0, 100, 100);
        let a = Rect::new(0, 0, 60, 100);
        let b = Rect::new(40, 0, 60, 100);
        assert!((d.coverage(&[a, b]) - 1.0).abs() < 1e-9);
        assert!((d.coverage(&[a]) - 0.6).abs() < 1e-9);
        assert_eq!(d.coverage(&[Rect::new(200, 200, 10, 10)]), 0.0);
        let partial = [Rect::new(-50, -50, 100, 100), Rect::new(50, 50, 100, 100)];
        assert!((d.coverage(&partial) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn union_and_intersection() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersection(&b), Some(Rect::new(5, 5, 5, 5)));
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert_eq!(Rect::bounds([&a, &b]), Rect::new(0, 0, 15, 15));
    }
}
