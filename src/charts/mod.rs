//! Minimal charts drawn with GPUI primitives. Lines are straight segments
//! between real end-of-day values (no smoothing). Axes use round steps with
//! the top tick above the maximum.

pub mod bars;
pub mod line;

use crate::domain::money::Cents;

/// Round tick values covering `[min, max]` with about `count` steps.
pub fn nice_ticks(min: Cents, max: Cents, count: usize, zero_based: bool) -> Vec<Cents> {
    let (mut lo, mut hi) = (min.min(max), min.max(max));
    if zero_based {
        lo = lo.min(0);
    }
    if hi == lo {
        let pad = (hi.abs() / 10).max(10_000);
        hi += pad;
        if !zero_based {
            lo -= pad;
        }
    }
    let range = (hi - lo) as f64;
    let rough = range / count.max(1) as f64;
    let mag = 10f64.powf(rough.log10().floor());
    let residual = rough / mag;
    let step_f = if residual <= 1.0 {
        1.0
    } else if residual <= 2.0 {
        2.0
    } else if residual <= 2.5 {
        2.5
    } else if residual <= 5.0 {
        5.0
    } else {
        10.0
    } * mag;
    // Money ticks land on whole currency units.
    let step = (step_f.round() as Cents).max(100);
    let start = (lo as f64 / step as f64).floor() as Cents * step;
    let mut end = (hi as f64 / step as f64).ceil() as Cents * step;
    if end <= hi {
        end += step;
    }
    let mut ticks = Vec::new();
    let mut t = start;
    while t <= end {
        ticks.push(t);
        t += step;
    }
    ticks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_round_and_cover_the_data() {
        let t = nice_ticks(5_000_000, 5_430_000, 4, false);
        assert!(*t.first().unwrap() <= 5_000_000);
        assert!(*t.last().unwrap() > 5_430_000);
        let step = t[1] - t[0];
        assert!(t.windows(2).all(|w| w[1] - w[0] == step));
        assert_eq!(step % 100, 0);

        let z = nice_ticks(0, 123_456, 4, true);
        assert_eq!(z[0], 0);
        assert!(*z.last().unwrap() > 123_456);

        let flat = nice_ticks(0, 0, 4, true);
        assert!(flat.len() >= 2);
    }
}
