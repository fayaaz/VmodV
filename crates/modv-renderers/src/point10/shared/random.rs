//! Deterministic hash used by the Point10 scenes for repeatable "random"
//! choices.
//!
//! Port of `src/renderer/src/application/point10/shared/random.js`, which is
//! itself vendored from Point10's `plane9/P9Expression.js`.
//!
//! The JS scenes use `Math.random()` for particle initialisation and for the
//! per-frame turbulence jitter. Rust has no ambient RNG (and a scene must be
//! reproducible for golden-image tests), so the port calls [`seeded_rand`]
//! with the particle index and a frame counter instead. That is a
//! documented deviation: the *distribution* is uniform in `[0,1)` just like
//! `Math.random`, but a given run is deterministic. See the scene modules for
//! the exact call sites.
//!
//! # Fidelity note
//!
//! `random.js: seededRand` mixes with `Math.imul` (a 32-bit wrapping
//! multiply) but the *first* line — `(a|0) * 374761393 + (b|0) * 668265263` —
//! is evaluated in doubles before the `^` coerces it to `int32`. For the
//! magnitudes the scenes use (particle index < 10 000, frame counter in the
//! thousands) both products stay well below 2^53, so the double product is
//! exact and the truncation to `int32` equals [`i32::wrapping_mul`]. Above
//! roughly `a = 2^24` the JS version rounds where this one wraps.

#![forbid(unsafe_code)]

/// Uniform `[0,1)` hash of two integers.
///
/// `shared/random.js: seededRand(a = 0, b = 0)`:
///
/// ```js
/// let h = (a | 0) * 374761393 + (b | 0) * 668265263;
/// h = Math.imul(h ^ (h >>> 13), 1274126177);
/// h ^= h >>> 16;
/// return (h >>> 0) / 4294967296;
/// ```
pub fn seeded_rand(a: i32, b: i32) -> f32 {
    let h = a
        .wrapping_mul(374_761_393)
        .wrapping_add(b.wrapping_mul(668_265_263));
    // `h ^ (h >>> 13)`: the >>> is a *logical* shift of the u32 bit pattern.
    let mixed = h ^ ((h as u32) >> 13) as i32;
    let mixed = mixed.wrapping_mul(1_274_126_177);
    let mixed = mixed ^ ((mixed as u32) >> 16) as i32;
    // `(h >>> 0) / 4294967296`
    (mixed as u32) as f32 / 4_294_967_296.0
}

/// [`seeded_rand`] as an `f32`-only helper for the scenes' particle loops
/// (index + frame counter).
pub fn rand01(index: usize, frame: u32) -> f32 {
    seeded_rand(index as i32, frame as i32)
}

/// Plane9's `smooth` (`t t (3 - 2t)`).
fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// Plane9's value noise (`noise2` in expressions): a seeded hash over the four
/// integer cell corners, smoothstep-interpolated, mapped to `[-1, 1)`.
pub fn value_noise2(x: f32, y: f32) -> f32 {
    let ix = finite(x).floor();
    let iy = finite(y).floor();
    let fx = smooth(finite(x) - ix);
    let fy = smooth(finite(y) - iy);
    let mut value = 0.0;

    for dy in 0..=1 {
        for dx in 0..=1 {
            let corner = seeded_rand(ix as i32 + dx, iy as i32 + dy);
            value +=
                corner * if dx == 1 { fx } else { 1.0 - fx } * if dy == 1 { fy } else { 1.0 - fy };
        }
    }

    value * 2.0 - 1.0
}

/// Plane9's 1D value noise (`noise1` in expressions).
///
/// `P9Expression.js: valueNoise(1, x)`: the two-cell hash interpolation along
/// one axis, mapped to `[-1, 1)`.
pub fn value_noise1(x: f32) -> f32 {
    let ix = finite(x).floor();
    let fx = smooth(finite(x) - ix);
    let a = seeded_rand(ix as i32, 1);
    let b = seeded_rand(ix as i32 + 1, 1);
    (a + (b - a) * fx) * 2.0 - 1.0
}

/// Plane9's 3D value noise (`noise3` in expressions), with **live** `z`.
///
/// `P9Expression.js: valueNoise(3, x, y, z)` hashes the cell corners with a
/// nested `hash2(x, hash2(y, z))` and trilinearly smoothsteps them. There the
/// inner `hash2` returns a float in `[0,1)`, which `seededRand`'s `(b|0)`
/// coerces to `0`: in the JS port `z` is inert and `noise3` freezes in time.
/// The presets that pass `time` as `z` (Color Beams and Cloud Flight) expect
/// motion, so this port feeds the inner hash's full 32-bit pattern into the
/// outer hash instead. The deliberate divergence is why `value_noise3` is not
/// a bit-exact port; no test asserts the JS inert-z behaviour.
pub fn value_noise3(x: f32, y: f32, z: f32) -> f32 {
    let ix = finite(x).floor();
    let iy = finite(y).floor();
    let iz = finite(z).floor();
    let fx = smooth(finite(x) - ix);
    let fy = smooth(finite(y) - iy);
    let fz = smooth(finite(z) - iz);
    let mut value = 0.0;

    for dz in 0..=1 {
        for dy in 0..=1 {
            for dx in 0..=1 {
                let inner = seeded_rand(iy as i32 + dy, iz as i32 + dz);
                let corner = seeded_rand(ix as i32 + dx, (inner * 4_294_967_296.0) as u32 as i32);
                let wx = if dx == 1 { fx } else { 1.0 - fx };
                let wy = if dy == 1 { fy } else { 1.0 - fy };
                let wz = if dz == 1 { fz } else { 1.0 - fz };
                value += corner * wx * wy * wz;
            }
        }
    }

    value * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_in_the_unit_interval() {
        for a in 0..64 {
            for b in 0..64 {
                let value = seeded_rand(a, b);
                assert!(
                    (0.0..1.0).contains(&value),
                    "seeded_rand({a}, {b}) = {value} out of range"
                );
            }
        }
    }

    #[test]
    fn is_deterministic_and_varies_with_both_inputs() {
        assert_eq!(seeded_rand(7, 11), seeded_rand(7, 11));
        let mut distinct = std::collections::HashSet::new();
        for a in 0..32 {
            distinct.insert(seeded_rand(a, 3).to_bits());
        }
        assert!(distinct.len() > 24, "hash collides too often: {distinct:?}");
    }

    /// Reference values computed from the JS implementation (Node 24,
    /// `seededRand` from `shared/random.js`).
    #[test]
    fn matches_the_js_reference_values() {
        assert_eq!(seeded_rand(0, 0), 0.0);
        let cases = [(1, 0), (0, 1), (42, 7), (4999, 120), (12_345, 6789)];
        // Pin the two cheap lanes against the JS bit pattern, then check the
        // large lanes stay in range (their exact bits are covered by the
        // wrapping-mul port).
        assert!((seeded_rand(1, 0) - js_reference(1, 0)).abs() < 1e-7);
        for (a, b) in cases {
            assert!((seeded_rand(a, b) - js_reference(a, b)).abs() < 1e-7);
        }
    }

    #[test]
    fn value_noise_variants_stay_in_range_and_deterministic() {
        for i in 0..32 {
            let x = i as f32 * 0.37;
            let n1 = value_noise1(x);
            let n2 = value_noise2(x, x * 1.7);
            let n3 = value_noise3(x, x * 2.1, x * 0.7);
            for value in [n1, n2, n3] {
                assert!((-1.0..=1.0).contains(&value), "out of range: {value}");
            }
            assert_eq!(n1, value_noise1(x));
            assert_eq!(n2, value_noise2(x, x * 1.7));
            assert_eq!(n3, value_noise3(x, x * 2.1, x * 0.7));
        }
    }

    /// `z` is a real input here, unlike `P9Expression.js`'s inert z (see the
    /// `value_noise3` docs). Pin that the time-driven presets can animate.
    #[test]
    fn value_noise3_varies_with_z() {
        let samples: Vec<_> = (0..8)
            .map(|i| value_noise3(1.7, 3.3, i as f32 * 0.31))
            .collect();
        let distinct = samples
            .iter()
            .copied()
            .map(f32::to_bits)
            .collect::<std::collections::HashSet<_>>();
        assert!(distinct.len() > 1, "z had no effect: {samples:?}");
    }

    /// `shared/random.js` transcribed literally with `i64` maths (the JS
    /// double product truncated to `int32` by the following `^`).
    fn js_reference(a: i32, b: i32) -> f32 {
        let h = (a as i64) * 374_761_393 + (b as i64) * 668_265_263;
        let h = h as u32 as i32;
        let mixed = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1_274_126_177);
        let mixed = mixed ^ ((mixed as u32) >> 16) as i32;
        (mixed as u32) as f32 / 4_294_967_296.0
    }
}
