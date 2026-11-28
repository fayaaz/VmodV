//! Reusable Plane9 ribbon model (`P9Nodes.js:1714-1848`).
//!
//! A Plane9 `Ribbons` node is a recorded head history, not a per-frame strip:
//! once per `EvaluationsPerSec` tick the vendor evaluates **only the head**
//! (point 0) of every ribbon and stores its `pos`, `width` and `col` in a ring
//! of `NumPoints` slots. The geometry then maps point `j` to the recorded slot
//! `(count - 1 - jc) mod capacity` with `jc = min(j, taken - 1)`, so:
//!
//! * point 0 is the **newest** sample and the last point is the **oldest**;
//! * during the **cold start** (before `NumPoints` ticks have elapsed) every
//!   point behind the head clamps to the oldest recorded sample;
//! * after `capacity` ticks the ring wraps and the trail holds exactly the last
//!   `capacity` samples.
//!
//! `t` starts at 0 and advances **before** the expression is evaluated, so the
//! first sample is at `1 / EvaluationsPerSec`, the second at `2 / eps`, and so
//! on. The update is a fixed-timestep accumulator: a frame that arrives after
//! several tick boundaries evaluates the head once per missed tick.
//!
//! Defaults when the expression does not assign them are `width = 0.5` and
//! `col = [1.0, 1.0, 1.0, 1.0]`, exactly like the vendor's
//! `env.locals.has('width') ? ... : 0.5` / `... : [1, 1, 1, 1]`.
//!
//! The audio bridge is **mono** (its channel argument is ignored), so a preset
//! whose two ribbon banks read `spectrum(0, ...)` and `spectrum(1, ...)` renders
//! mirror twins rather than stereo channels. That is a documented deviation of
//! the port, recorded by the scene; this helper only stores whatever the
//! caller's expression produced.
//!
//! # Geometry
//!
//! Each ribbon is a triangle strip of `np` points with two vertices per point
//! (`nr * np * 2` vertices, `(np - 1) * 6 * nr` indices). `uv.x` alternates
//! 0/1 across the strip and `uv.y = j/(np-1)` runs from the head (0) to the tail
//! (1). [`write_ribbon_bank`] applies the vendor's tangent basis: the tangent
//! is taken from the neighbours `j - 1` and `j + 1` (clamped), the side vector
//! is `cross(tangent, up) = (-tz, 0, tx)`, and the half-width is
//! `width * 0.5`. Only the tangent's x and z components are normalised, exactly
//! as `P9Nodes.js:1828-1833` does.

#![forbid(unsafe_code)]

use bytemuck::{Pod, Zeroable};

/// One evaluated head sample of a ribbon expression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RibbonSample {
    /// `pos` from the expression.
    pub position: [f32; 3],
    /// `width` from the expression; `None` uses the vendor default `0.5`.
    pub width: Option<f32>,
    /// `col` from the expression; `None` uses the vendor default `(1,1,1,1)`.
    pub color: Option<[f32; 4]>,
}

impl RibbonSample {
    /// A sample at `position` that leaves `width`/`col` at their defaults.
    pub const fn at(position: [f32; 3]) -> Self {
        Self {
            position,
            width: None,
            color: None,
        }
    }
}

impl Default for RibbonSample {
    fn default() -> Self {
        Self::at([0.0; 3])
    }
}

/// The recorded head history of one ribbon.
///
/// Capacity is `NumPoints`; `evaluations_per_sec` is the node's
/// `EvaluationsPerSec`. See the module docs for the newest-first mapping and the
/// cold-start clamp.
#[derive(Clone, Debug)]
pub struct RibbonHistory {
    capacity: usize,
    evaluations_per_sec: f32,
    tick_time: f32,
    count: usize,
    positions: Vec<[f32; 3]>,
    widths: Vec<f32>,
    colors: Vec<[f32; 4]>,
}

impl RibbonHistory {
    /// An empty history of `capacity` slots evaluating at `evaluations_per_sec`.
    pub fn new(capacity: usize, evaluations_per_sec: f32) -> Self {
        let capacity = capacity.max(1);
        let evaluations_per_sec = if evaluations_per_sec.is_finite() && evaluations_per_sec > 0.0 {
            evaluations_per_sec
        } else {
            30.0
        };
        Self {
            capacity,
            evaluations_per_sec,
            tick_time: 0.0,
            count: 0,
            positions: vec![[0.0; 3]; capacity],
            widths: vec![0.0; capacity],
            colors: vec![[0.0; 4]; capacity],
        }
    }

    /// The ring size (`NumPoints`).
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The evaluations per second (`EvaluationsPerSec`).
    pub fn evaluations_per_sec(&self) -> f32 {
        self.evaluations_per_sec
    }

    /// How many head samples have been recorded (may exceed [`Self::capacity`]).
    pub fn count(&self) -> usize {
        self.count
    }

    /// Whether no tick has been evaluated yet (the cold start).
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Advance the fixed-timestep clock to `time`, evaluating the head once per
    /// crossed tick.
    ///
    /// The expression receives the tick time (`t` starts at 0 and is advanced
    /// **before** evaluation), matching `P9Nodes.js:1780-1792`. A frame that
    /// spans several ticks catches up, so a dropped frame cannot shorten the
    /// trail.
    pub fn advance(&mut self, time: f32, mut evaluate: impl FnMut(f32) -> RibbonSample) {
        if !time.is_finite() {
            return;
        }
        let evaluations_per_sec = self.evaluations_per_sec;
        let tick_dt = 1.0 / evaluations_per_sec;
        let capacity = self.capacity;
        while self.tick_time + tick_dt <= time {
            self.tick_time += tick_dt;
            let sample = evaluate(self.tick_time);
            let slot = self.count % capacity;
            self.positions[slot] = sample.position;
            self.widths[slot] = sample.width.unwrap_or(0.5);
            self.colors[slot] = sample.color.unwrap_or([1.0, 1.0, 1.0, 1.0]);
            self.count += 1;
        }
    }

    /// The sample for geometry point `point`.
    ///
    /// Point 0 is the **newest** head sample, point 1 the previous head, and so
    /// on. Once `point` reaches the number of recorded samples the cold tail
    /// clamps to the **oldest** recorded sample. Returns `None` before the first
    /// tick.
    pub fn point(&self, point: usize) -> Option<RibbonSample> {
        if self.count == 0 {
            return None;
        }
        let count = self.count;
        let capacity = self.capacity;
        let taken = count.min(capacity);
        let clamped = point.min(taken - 1);
        let slot = (count - 1 - clamped) % capacity;
        Some(RibbonSample {
            position: self.positions[slot],
            width: Some(self.widths[slot]),
            color: Some(self.colors[slot]),
        })
    }
}

/// One interleaved ribbon vertex (`position`, `color`, `uv`).
///
/// Attribute locations 0/1/2, matching the vendor's `iPosition`, the RGBA
/// `iColor` attribute `P9Nodes.js:1720-1723` adds for `USE_COLOR_ALPHA`, and
/// `iTexCoord`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct RibbonVertex {
    /// World position after the tangent-width offset.
    pub position: [f32; 3],
    /// `iColor` — the expression's `col`. A `col.a`-only expression yields
    /// `[0,0,0,i]` in the vendor env; the `[1,1,1,1]` default applies only when
    /// the expression never assigns `col`.
    pub color: [f32; 4],
    /// `iTexCoord` — `u` in `{0, 1}`, `v` along the trail.
    pub uv: [f32; 2],
}

impl RibbonVertex {
    /// Buffer layout for `@location(0..2)`.
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 12,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 28,
                    shader_location: 2,
                },
            ],
        }
    }
}

/// The vertex count of `nr` ribbons with `np` points: `nr * np * 2`.
pub fn ribbon_vertex_count(nr: usize, np: usize) -> usize {
    nr * np * 2
}

/// The index count of `nr` ribbons with `np` points: `(np - 1) * 6 * nr`.
pub fn ribbon_index_count(nr: usize, np: usize) -> usize {
    (np - 1) * 6 * nr
}

/// The vendor UV ladder: `uv.y = j/(np-1)` for point `j`, `uv.x` 0 then 1.
pub fn ribbon_uv(j: usize, np: usize) -> [f32; 2] {
    [0.0, j as f32 / (np - 1).max(1) as f32]
}

/// The vendor index buffer: `a, b, c, c, b, d` per point segment and ribbon.
pub fn ribbon_indices(nr: usize, np: usize) -> Vec<u32> {
    let mut indices = Vec::with_capacity(ribbon_index_count(nr, np));
    for ribbon in 0..nr {
        for j in 0..np.saturating_sub(1) {
            let a = ((ribbon * np + j) * 2) as u32;
            indices.extend_from_slice(&[a, a + 1, a + 2, a + 2, a + 1, a + 3]);
        }
    }
    indices
}

/// Widen one bank's histories into its interleaved vertex buffer.
///
/// `histories[r]` is ribbon `r`. An empty history (before the first tick) emits
/// zero-width vertices at the origin so the cold frame draws nothing, exactly
/// like the vendor's zeroed `Float32Array` geometry. The tangent basis is
/// `P9Nodes.js:1824-1841`: neighbours `j - 1` / `j + 1` clamped, the side
/// vector `cross(tangent, up) = (-tz, 0, tx)`, and half-width `width * 0.5`.
pub fn write_ribbon_bank(
    histories: &[RibbonHistory],
    nr: usize,
    np: usize,
    vertices: &mut Vec<RibbonVertex>,
) {
    vertices.reserve(nr * np * 2);
    for history in histories.iter().take(nr) {
        for j in 0..np {
            if history.is_empty() {
                let uv = [0.0, j as f32 / (np - 1).max(1) as f32];
                vertices.push(RibbonVertex {
                    position: [0.0; 3],
                    color: [0.0; 4],
                    uv,
                });
                vertices.push(RibbonVertex {
                    position: [0.0; 3],
                    color: [0.0; 4],
                    uv: [1.0, uv[1]],
                });
                continue;
            }
            let head = history.point(j).unwrap_or_default();
            let previous = history.point(j.saturating_sub(1)).unwrap_or_default();
            let next = history.point((j + 1).min(np - 1)).unwrap_or_default();
            let mut tx = next.position[0] - previous.position[0];
            let ty = next.position[1] - previous.position[1];
            let mut tz = next.position[2] - previous.position[2];
            let length = (tx * tx + ty * ty + tz * tz).sqrt();
            if length > 1.0e-9 {
                // The vendor normalises only x and z and leaves y untouched;
                // y never reaches the side vector, so this is exact.
                tx /= length;
                tz /= length;
            } else {
                tx = 1.0;
                tz = 0.0;
            }
            let side = [-tz, 0.0, tx];
            let half_width = head.width.unwrap_or(0.5) * 0.5;
            let color = head.color.unwrap_or([1.0, 1.0, 1.0, 1.0]);
            let uv = [0.0, j as f32 / (np - 1).max(1) as f32];
            vertices.push(RibbonVertex {
                position: [
                    head.position[0] + side[0] * half_width,
                    head.position[1],
                    head.position[2] + side[2] * half_width,
                ],
                color,
                uv,
            });
            vertices.push(RibbonVertex {
                position: [
                    head.position[0] - side[0] * half_width,
                    head.position[1],
                    head.position[2] - side[2] * half_width,
                ],
                color,
                uv: [1.0, uv[1]],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_at(time: f32) -> RibbonSample {
        RibbonSample::at([time, time * 2.0, time * 3.0])
    }

    #[test]
    fn ticks_at_thirty_hertz_and_catches_up() {
        let mut history = RibbonHistory::new(110, 30.0);
        history.advance(0.0, sample_at);
        assert!(history.is_empty(), "no tick has been crossed at t = 0");

        history.advance(0.04, sample_at);
        assert_eq!(history.count(), 1);
        let first = history.point(0).unwrap();
        assert!((first.position[0] - 1.0 / 30.0).abs() < 1.0e-6);

        history.advance(0.07, sample_at);
        assert_eq!(history.count(), 2);

        // A frame that spans several boundaries evaluates every missed tick.
        history.advance(2.0, sample_at);
        assert_eq!(history.count(), 60);
    }

    #[test]
    fn point_zero_is_newest_and_the_cold_tail_clamps_to_the_oldest() {
        let mut history = RibbonHistory::new(110, 30.0);
        history.advance(0.04, sample_at);
        let only = history.point(0).unwrap();
        assert!((0..110).all(|point| history.point(point) == Some(only)));

        history.advance(0.07, sample_at);
        let newest = history.point(0).unwrap();
        let previous = history.point(1).unwrap();
        assert!(newest.position[0] > previous.position[0]);
        assert!((2..110).all(|point| history.point(point) == Some(previous)));
    }

    #[test]
    fn the_ring_wraps_at_capacity() {
        let mut history = RibbonHistory::new(110, 30.0);
        history.advance(120.0 / 30.0, sample_at);
        assert_eq!(history.count(), 120);

        let head = history.point(0).unwrap();
        assert!((head.position[0] - 120.0 / 30.0).abs() < 1.0e-5);
        let tail = history.point(109).unwrap();
        assert!((tail.position[0] - 11.0 / 30.0).abs() < 1.0e-5);
        assert_eq!(history.point(200), Some(tail));
    }

    #[test]
    fn unset_width_and_colour_use_the_vendor_defaults() {
        let mut history = RibbonHistory::new(4, 30.0);
        history.advance(1.0 / 30.0, |time| RibbonSample::at([time, 0.0, 0.0]));
        let point = history.point(0).unwrap();
        assert_eq!(point.width, Some(0.5));
        assert_eq!(point.color, Some([1.0, 1.0, 1.0, 1.0]));
    }

    #[test]
    fn geometry_counts_and_uv_ladder_match_the_vendor() {
        assert_eq!(ribbon_vertex_count(100, 110), 22_000);
        assert_eq!(ribbon_index_count(100, 110), 65_400);
        let indices = ribbon_indices(2, 3);
        assert_eq!(
            indices,
            vec![
                0, 1, 2, 2, 1, 3, 2, 3, 4, 4, 3, 5, 6, 7, 8, 8, 7, 9, 8, 9, 10, 10, 9, 11
            ]
        );
        assert_eq!(ribbon_uv(0, 110), [0.0, 0.0]);
        assert!((ribbon_uv(109, 110)[1] - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn tangent_basis_splits_the_width_across_the_side_vector() {
        // A straight trail along +x has tangent (1, 0, 0) and side (0, 0, 1).
        let mut history = RibbonHistory::new(3, 30.0);
        history.advance(3.0 / 30.0, |time| RibbonSample {
            position: [time, 0.0, 0.0],
            width: Some(0.5),
            color: Some([1.0, 1.0, 1.0, 1.0]),
        });
        let mut vertices = Vec::new();
        write_ribbon_bank(std::slice::from_ref(&history), 1, 3, &mut vertices);
        assert_eq!(vertices.len(), 6);
        // The trail runs backwards from the newest point, so the tangent is
        // -x, the side vector is (0, 0, -1), and half of the 0.5 width puts the
        // two vertices at ∓0.25 in z around the head.
        assert!((vertices[0].position[2] + 0.25).abs() < 1.0e-6);
        assert!((vertices[1].position[2] - 0.25).abs() < 1.0e-6);
        assert_eq!(vertices[0].uv, [0.0, 0.0]);
        assert_eq!(vertices[1].uv, [1.0, 0.0]);
    }
}
