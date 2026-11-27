//! Reusable CPU mesh modifiers for the Plane9 mesh-node presets.
//!
//! `Things/Spiky.xml` is the first ported preset whose scene graph is a mesh
//! modifier chain rather than a shader-only graph:
//!
//! ```text
//! Cube1 -> Subdivide2 -> SelectMesh1 -> Extrude2 -> Randomize1
//!       -> Subdivide1 -> TransformEx1 -> MeshObject1
//! ```
//!
//! Plane9's own `P9Nodes.js` implements those nodes as passthroughs
//! (`P9Nodes.js:2104-2124`), so the geometry here is a reconstruction from the
//! XML port values, not a line-for-line port. Every operation documents the
//! node/port it reproduces.
//!
//! # Pure CPU, no GPU
//!
//! Nothing in this module touches `wgpu`: it builds an indexed mesh, and the
//! owning scene uploads [`IndexedMesh::to_render_mesh`] through
//! [`crate::point10::scenes::cube_mesh::MeshGpu`]. That keeps the whole chain
//! unit-testable on CI without an adapter.
//!
//! # Why `TransformEx` is not here
//!
//! `TransformEx1`'s `Scale 2 2 1` is an *object* transform in Plane9's graph,
//! not a mesh modifier: the shader's tip mask reads the pre-transform local
//! position (`iPosition`). Baking that scale into the vertex data would move
//! the whole surface past `d > 0.9` and turn the ball into a solid red
//! sphere, so the scene composes it into the model matrix
//! ([`super::spiky::build_spiky_mesh`] deliberately stops before it).
//!
//! # Subdivision smoothing
//!
//! `Subdivide`'s `Smootness` is a 0..1 blend toward the Loop limit surface.
//! New edge vertices use the full Loop rule
//! (`3/8 (p1 + p2) + 1/8 (opposite corners)`); existing vertices blend toward
//! their one-ring average with a quarter weight at `smoothness = 1`. The
//! quarter weight is deliberate: the Spiky chain extrudes a 0.88 cone per face
//! and then subdivides twice, and full Loop vertex motion would collapse every
//! apex back inside the tip mask. The XML's `Smootness 1` therefore means
//! "round the new edge vertices, keep the silhouette", which is what the
//! vendor preset looks like.
//!
//! # Port map
//!
//! | XML node | Ports | Function |
//! | --- | --- | --- |
//! | `Subdivide` | `NumSteps`, `Smootness`, `Selection` | [`subdivide`] |
//! | `SelectMesh` | `SelectionType`, `Logic`, `Invert`, `Output`, `RandomFactor`, `RandomSeed` | [`select_faces`] |
//! | `Extrude` | `NumSteps`, `Amount`, `Direction`, `LocalScale` | [`extrude`] |
//! | `Randomize` | `Selection`, `Direction`, `Amount`, `RandomSeed` | [`randomize`] |
//! | `TransformEx` | `Scale` | **not here** — the scene's model matrix |
//! | `Cube` | `Size`, tessellation | [`from_box`] / `scenes::cube_mesh::build_box` |

#![forbid(unsafe_code)]

use std::collections::HashMap;

use crate::point10::scenes::cube_mesh::{Mesh, Vertex};
use crate::point10::shared::random::seeded_rand;

/// A welded, triangle-indexed mesh (`positions` + `triangles`).
///
/// Faces are triangles: the cube's 12 triangles stay triangles through every
/// modifier, which is what Plane9's renderer consumes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexedMesh {
    /// Unique vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Triangle corner indices into [`IndexedMesh::positions`].
    pub triangles: Vec<[u32; 3]>,
}

impl IndexedMesh {
    /// Number of triangles.
    pub fn face_count(&self) -> usize {
        self.triangles.len()
    }

    /// `(minimum, maximum)` distance from the origin.
    pub fn radius_range(&self) -> (f32, f32) {
        let mut min = f32::INFINITY;
        let mut max = 0.0_f32;
        for position in &self.positions {
            let radius = length(*position);
            min = min.min(radius);
            max = max.max(radius);
        }
        if self.positions.is_empty() {
            (0.0, 0.0)
        } else {
            (min, max)
        }
    }

    /// Build a renderable mesh with smooth normals and a spherical UV.
    ///
    /// The Spiky fragment shader only uses `iTexCoord` for the screen-space
    /// bump derivative, so a continuous spherical mapping is enough; the
    /// position/normal inputs are what the tip mask and lighting read.
    pub fn to_render_mesh(&self) -> Mesh {
        let mut normals = vec![[0.0_f32; 3]; self.positions.len()];
        for triangle in &self.triangles {
            let [a, b, c] = triangle.map(|index| self.positions[index as usize]);
            let face = cross(sub(b, a), sub(c, a));
            for index in triangle {
                let normal = &mut normals[*index as usize];
                *normal = add(*normal, face);
            }
        }

        let vertices = self
            .positions
            .iter()
            .zip(normals)
            .map(|(position, normal)| Vertex {
                position: *position,
                normal: normalise(normal),
                uv: spherical_uv(*position),
            })
            .collect();

        Mesh {
            vertices,
            indices: self
                .triangles
                .iter()
                .flat_map(|triangle| triangle.iter().copied())
                .collect(),
        }
    }
}

/// `Subdivide` ports (`NumSteps`, `Smootness`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubdivideParams {
    /// `NumSteps` — midpoint split passes.
    pub num_steps: u32,
    /// `Smootness` (sic) — 0 keeps the raw midpoints, 1 rounds the surface.
    pub smoothness: f32,
}

/// `SelectMesh` ports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectParams {
    /// `SelectionType`: `4` is every face.
    pub selection_type: u32,
    /// `Logic`: `1` is union.
    pub logic: u32,
    /// `Invert` — return the complement instead.
    pub invert: bool,
    /// `Output` — `1` outputs the selected (as opposed to deleted) faces.
    pub output: u32,
    /// `RandomFactor` — `1` keeps every selected face.
    pub random_factor: f32,
    /// `RandomSeed` for the random subset.
    pub random_seed: u32,
}

/// `Extrude` ports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtrudeParams {
    /// `NumSteps` — the number of rings between the base and the cap.
    pub num_steps: u32,
    /// `Amount` — distance along the face normal.
    pub amount: f32,
    /// `Direction`: `0` is the face normal.
    pub direction: u32,
    /// `LocalScale` — cap scale; `0` collapses the cap to one apex.
    pub local_scale: f32,
}

/// `Randomize` ports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RandomizeParams {
    /// `Amount` — the displacement bound.
    pub amount: f32,
    /// `Direction`: `0` is the vertex normal.
    pub direction: u32,
    /// `RandomSeed`.
    pub random_seed: u32,
}

/// Weld a render mesh's split vertices by position.
///
/// `cube_mesh::build_box` keeps four vertices per face (24 total) so each face
/// owns its UVs; a modifier chain needs one shared vertex per corner (8) or a
/// midpoint split would tear the cube open.
pub fn weld(mesh: &Mesh) -> IndexedMesh {
    let mut lookup: HashMap<[i64; 3], u32> = HashMap::new();
    let mut positions = Vec::new();
    let mut remap = Vec::with_capacity(mesh.vertices.len());

    for vertex in &mesh.vertices {
        let key = quantise(vertex.position);
        let index = *lookup.entry(key).or_insert_with(|| {
            positions.push(vertex.position);
            (positions.len() - 1) as u32
        });
        remap.push(index);
    }

    let triangles = mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            [
                remap[triangle[0] as usize],
                remap[triangle[1] as usize],
                remap[triangle[2] as usize],
            ]
        })
        .collect();

    IndexedMesh {
        positions,
        triangles,
    }
}

/// `Cube1` -> a welded indexed cube.
pub fn from_box(width: f32, height: f32, depth: f32) -> IndexedMesh {
    weld(&crate::point10::scenes::cube_mesh::build_box(
        width, height, depth,
    ))
}

/// `Subdivide`: loop/midpoint subdivision.
///
/// Each step replaces every triangle with four, so edge resolution doubles and
/// the face count quadruples. A closed mesh gains one vertex per unique edge
/// per step (the cube's 8 vertices and 18 edges become 26 vertices and 48
/// triangles after `NumSteps 1`).
pub fn subdivide(mesh: &IndexedMesh, params: SubdivideParams) -> IndexedMesh {
    let smoothness = if params.smoothness.is_finite() {
        params.smoothness.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut current = mesh.clone();

    for _ in 0..params.num_steps {
        current = subdivide_once(&current, smoothness);
    }

    current
}

/// One midpoint-split pass.
fn subdivide_once(mesh: &IndexedMesh, smoothness: f32) -> IndexedMesh {
    // Undirected edge -> (midpoint position accumulator, opposite vertices).
    let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); mesh.positions.len()];
    for triangle in &mesh.triangles {
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            if !neighbours[a as usize].contains(&b) {
                neighbours[a as usize].push(b);
            }
            if !neighbours[b as usize].contains(&a) {
                neighbours[b as usize].push(a);
            }
        }
    }

    let mut positions: Vec<[f32; 3]> = mesh
        .positions
        .iter()
        .zip(&neighbours)
        .map(|(position, ring)| {
            if ring.is_empty() {
                return *position;
            }
            let mut average = [0.0_f32; 3];
            for index in ring {
                average = add(average, mesh.positions[*index as usize]);
            }
            let count = ring.len() as f32;
            average = scale(average, 1.0 / count);
            // `Smootness` blends toward the limit position; the quarter weight
            // keeps an extruded apex in place (see the module docs).
            lerp(*position, average, 0.25 * smoothness)
        })
        .collect();

    // Edge -> (midpoint index, opposite vertices).
    let mut edges: HashMap<(u32, u32), (u32, Vec<u32>)> = HashMap::new();
    let mut triangles = Vec::with_capacity(mesh.triangles.len() * 4);

    for triangle in &mesh.triangles {
        let corners = *triangle;
        let mut midpoints = [0_u32; 3];
        for (slot, (a, b)) in [(0, (0, 1)), (1, (1, 2)), (2, (2, 0))] {
            let (i, j) = (corners[a], corners[b]);
            let opposite = corners[3 - a - b];
            let key = ordered_edge(i, j);
            let (index, opposites) = edges.entry(key).or_insert_with(|| {
                let index = positions.len() as u32;
                positions.push([0.0; 3]);
                (index, Vec::new())
            });
            opposites.push(opposite);
            midpoints[slot] = *index;
        }

        let [ab, bc, ca] = midpoints;
        triangles.push([corners[0], ab, ca]);
        triangles.push([ab, corners[1], bc]);
        triangles.push([ca, bc, corners[2]]);
        triangles.push([ab, bc, ca]);
    }

    // Position every midpoint once, now that its opposite vertices are known.
    for ((a, b), (index, opposites)) in &edges {
        let p0 = mesh.positions[*a as usize];
        let p1 = mesh.positions[*b as usize];
        let raw = scale(add(p0, p1), 0.5);
        let mut opposite_sum = [0.0_f32; 3];
        for opposite in opposites {
            opposite_sum = add(opposite_sum, mesh.positions[*opposite as usize]);
        }
        let loop_position = if opposites.is_empty() {
            raw
        } else {
            add(
                scale(add(p0, p1), 3.0 / 8.0),
                scale(opposite_sum, 1.0 / 8.0),
            )
        };
        positions[*index as usize] = lerp(raw, loop_position, smoothness);
    }

    IndexedMesh {
        positions,
        triangles,
    }
}

/// `SelectMesh`: keep or invert a face selection.
///
/// Only `SelectionType 4` (all faces) is used by Spiky; `Logic 1` is union and
/// `Output 1` emits the selection. `RandomFactor 1` keeps every selected face,
/// and a smaller factor deterministically drops faces from the seeded subset.
pub fn select_faces(mesh: &IndexedMesh, params: SelectParams) -> IndexedMesh {
    let all = params.selection_type == 4 || params.selection_type == 0;
    let factor = if params.random_factor.is_finite() {
        params.random_factor.clamp(0.0, 1.0)
    } else {
        1.0
    };

    let keep: Vec<bool> = mesh
        .triangles
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let selected = all
                && (factor >= 1.0 || seeded_rand(params.random_seed as i32, index as i32) < factor);
            if params.invert { !selected } else { selected }
        })
        .collect();

    // `Output 0` emits the unselected half instead.
    let emit = if params.output == 0 {
        keep.iter().map(|keep| !keep).collect()
    } else {
        keep
    };

    let mut remap: HashMap<u32, u32> = HashMap::new();
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    for (index, triangle) in mesh.triangles.iter().enumerate() {
        if !emit[index] {
            continue;
        }
        let mapped = triangle.map(|corner| {
            *remap.entry(corner).or_insert_with(|| {
                positions.push(mesh.positions[corner as usize]);
                (positions.len() - 1) as u32
            })
        });
        triangles.push(mapped);
    }

    IndexedMesh {
        positions,
        triangles,
    }
}

/// `Extrude`: push every face along its normal and taper the cap.
///
/// `LocalScale 0` collapses the cap to a single apex, turning each face into a
/// cone (Spiky's spike); a non-zero value keeps a smaller copy of the face at
/// the cap. `NumSteps 2` emits one intermediate ring before the apex.
///
/// The base face is retained: with back-face culling the spike's outside is
/// what the camera sees, and keeping the base keeps every triangle wound
/// outward without a separate cap-orientation pass.
pub fn extrude(mesh: &IndexedMesh, params: ExtrudeParams) -> IndexedMesh {
    let steps = params.num_steps.max(1);
    let amount = if params.amount.is_finite() {
        params.amount
    } else {
        0.0
    };
    let local_scale = if params.local_scale.is_finite() {
        params.local_scale
    } else {
        1.0
    };

    let mut positions = Vec::new();
    let mut triangles = Vec::new();

    for face in &mesh.triangles {
        let base = face.map(|index| mesh.positions[index as usize]);
        let centroid = scale(add(add(base[0], base[1]), base[2]), 1.0 / 3.0);
        let normal = normalise(cross(sub(base[1], base[0]), sub(base[2], base[0])));

        // Direction 0 is the face normal; other directions are not used by
        // Spiky and fall back to the same normal.
        let direction = normal;

        let base_start = positions.len() as u32;
        positions.extend_from_slice(&base);

        // ring[ring][corner] vertex indices; ring 0 is the base face.
        let mut rings: Vec<[u32; 3]> = Vec::with_capacity(steps as usize + 1);
        rings.push([base_start, base_start + 1, base_start + 2]);
        let mut apex: Option<u32> = None;

        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let cap_scale = 1.0 + (local_scale - 1.0) * t;
            if step == steps && local_scale == 0.0 {
                // All three cap vertices land on the same point: emit one apex.
                let apex_position = add(centroid, scale(direction, amount * t));
                let index = positions.len() as u32;
                positions.push(apex_position);
                apex = Some(index);
                break;
            }
            let mut ring = [0_u32; 3];
            for (corner, vertex) in base.iter().enumerate() {
                let tangential = sub(*vertex, centroid);
                let ring_position = add(
                    add(centroid, scale(direction, amount * t)),
                    scale(tangential, cap_scale),
                );
                ring[corner] = positions.len() as u32;
                positions.push(ring_position);
            }
            rings.push(ring);
        }

        // Base triangle (unchanged, wound outward).
        triangles.push([base_start, base_start + 1, base_start + 2]);

        for pair in rings.windows(2) {
            let lower = pair[0];
            let upper = pair[1];
            for edge in 0..3 {
                let next = (edge + 1) % 3;
                triangles.push([lower[edge], lower[next], upper[next]]);
                triangles.push([lower[edge], upper[next], upper[edge]]);
            }
        }

        // Connect the last full ring to the collapsed apex.
        if let Some(apex) = apex {
            let ring = *rings.last().expect("a base ring exists");
            for edge in 0..3 {
                let next = (edge + 1) % 3;
                triangles.push([ring[edge], ring[next], apex]);
            }
        }
    }

    IndexedMesh {
        positions,
        triangles,
    }
}

/// `Randomize`: deterministic normal-direction displacement in
/// `[-Amount, +Amount)`.
///
/// Plane9's node is a per-vertex jitter: the seeded hash is uniform in `[0,1)`
/// and `Direction 0` displaces along the vertex normal, so the sign comes from
/// remapping the hash to `[-1, 1)`. Outward-only jitter would bake a bias the
/// XML's `Amount` does not specify.
pub fn randomize(mesh: &IndexedMesh, params: RandomizeParams) -> IndexedMesh {
    let amount = if params.amount.is_finite() {
        params.amount.abs()
    } else {
        0.0
    };
    let mut normals = vec![[0.0_f32; 3]; mesh.positions.len()];
    for triangle in &mesh.triangles {
        let [a, b, c] = triangle.map(|index| mesh.positions[index as usize]);
        let face = cross(sub(b, a), sub(c, a));
        for index in triangle {
            let normal = &mut normals[*index as usize];
            *normal = add(*normal, face);
        }
    }

    let positions = mesh
        .positions
        .iter()
        .enumerate()
        .map(|(index, position)| {
            let normal = normalise(normals[index]);
            let jitter = seeded_rand(params.random_seed as i32, index as i32) * 2.0 - 1.0;
            add(*position, scale(normal, jitter * amount))
        })
        .collect();

    IndexedMesh {
        positions,
        triangles: mesh.triangles.clone(),
    }
}

// ---------------------------------------------------------------------------
// Vector helpers
// ---------------------------------------------------------------------------

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], factor: f32) -> [f32; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

fn normalise(a: [f32; 3]) -> [f32; 3] {
    let length = length(a).max(1e-9);
    scale(a, 1.0 / length)
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    add(a, scale(sub(b, a), t.clamp(0.0, 1.0)))
}

fn quantise(position: [f32; 3]) -> [i64; 3] {
    position.map(|value| (value * 100_000.0).round() as i64)
}

fn ordered_edge(a: u32, b: u32) -> (u32, u32) {
    if a <= b { (a, b) } else { (b, a) }
}

fn spherical_uv(position: [f32; 3]) -> [f32; 2] {
    let radius = length(position).max(1e-9);
    let u = position[2].atan2(position[0]) / std::f32::consts::TAU + 0.5;
    let v = (position[1] / radius).clamp(-1.0, 1.0).acos() / std::f32::consts::PI;
    [u, v]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn face_keys(mesh: &IndexedMesh) -> BTreeSet<[[i64; 3]; 3]> {
        mesh.triangles
            .iter()
            .map(|triangle| {
                let mut corners: Vec<_> = triangle
                    .map(|index| quantise(mesh.positions[index as usize]))
                    .to_vec();
                corners.sort_unstable();
                [corners[0], corners[1], corners[2]]
            })
            .collect()
    }

    fn cube() -> IndexedMesh {
        from_box(1.0, 1.0, 1.0)
    }

    #[test]
    fn the_cube_welds_to_eight_vertices_and_twelve_triangles() {
        let mesh = cube();
        assert_eq!(mesh.positions.len(), 8);
        assert_eq!(mesh.triangles.len(), 12);
        let (min, max) = mesh.radius_range();
        assert!((max - (3.0_f32 / 4.0).sqrt()).abs() < 1e-6, "{max}");
        assert!((min - (3.0_f32 / 4.0).sqrt()).abs() < 1e-6, "{min}");
    }

    #[test]
    fn subdivide_quadruples_faces_per_step_and_adds_one_vertex_per_edge() {
        let once = subdivide(
            &cube(),
            SubdivideParams {
                num_steps: 1,
                smoothness: 0.0,
            },
        );
        assert_eq!(once.positions.len(), 8 + 18);
        assert_eq!(once.face_count(), 48);

        let twice = subdivide(
            &cube(),
            SubdivideParams {
                num_steps: 2,
                smoothness: 0.0,
            },
        );
        assert_eq!(twice.face_count(), 48 * 4);
    }

    #[test]
    fn subdivision_shares_midpoints_without_duplicate_interior_faces() {
        let once = subdivide(
            &cube(),
            SubdivideParams {
                num_steps: 1,
                smoothness: 0.0,
            },
        );
        let mut edges: HashMap<(u32, u32), usize> = HashMap::new();
        for triangle in &once.triangles {
            for (a, b) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                *edges.entry(ordered_edge(a, b)).or_insert(0) += 1;
            }
        }
        // A welded, closed cube has V=26, F=48, so E=V+F-2=72; every edge is
        // shared by exactly two faces (no torn seams, no doubled interiors).
        assert_eq!(edges.len(), 72);
        assert!(edges.values().all(|&uses| uses == 2), "non-manifold edge");
        assert_eq!(face_keys(&once).len(), once.face_count(), "duplicate face");
    }

    #[test]
    fn subdivision_smooths_toward_the_limit_surface_without_collapsing() {
        let raw = subdivide(
            &cube(),
            SubdivideParams {
                num_steps: 1,
                smoothness: 0.0,
            },
        );
        let smooth = subdivide(
            &cube(),
            SubdivideParams {
                num_steps: 1,
                smoothness: 1.0,
            },
        );
        let raw_max = raw.radius_range().1;
        let smooth_max = smooth.radius_range().1;
        assert!(smooth_max < raw_max, "{smooth_max} !< {raw_max}");
        assert!(smooth_max > 0.5, "smoothed cube collapsed to {smooth_max}");
        // The one-ring order survives: every triangle is still non-degenerate.
        assert!(smooth.triangles.iter().all(|triangle| {
            let [a, b, c] = triangle.map(|index| smooth.positions[index as usize]);
            length(cross(sub(b, a), sub(c, a))) > 1e-6
        }));
    }

    #[test]
    fn select_faces_keeps_all_and_invert_returns_the_complement() {
        let mesh = cube();
        let all = select_faces(
            &mesh,
            SelectParams {
                selection_type: 4,
                logic: 1,
                invert: false,
                output: 1,
                random_factor: 1.0,
                random_seed: 1,
            },
        );
        assert_eq!(all.face_count(), 12);

        let inverted = select_faces(
            &mesh,
            SelectParams {
                selection_type: 4,
                logic: 1,
                invert: true,
                output: 1,
                random_factor: 1.0,
                random_seed: 1,
            },
        );
        assert_eq!(inverted.face_count(), 0);
    }

    #[test]
    fn invert_is_a_true_complement_of_a_partial_face_selection() {
        let mesh = cube();
        let params = SelectParams {
            selection_type: 4,
            logic: 1,
            invert: false,
            output: 1,
            random_factor: 0.5,
            random_seed: 1,
        };
        let kept = select_faces(&mesh, params);
        let inverted = select_faces(
            &mesh,
            SelectParams {
                invert: true,
                ..params
            },
        );
        assert!(kept.face_count() > 0 && kept.face_count() < mesh.face_count());
        let kept_keys = face_keys(&kept);
        let inverted_keys = face_keys(&inverted);
        assert!(kept_keys.is_disjoint(&inverted_keys), "overlapping halves");
        let union: BTreeSet<_> = kept_keys.union(&inverted_keys).cloned().collect();
        assert_eq!(
            union,
            face_keys(&mesh),
            "the two halves must partition the mesh"
        );
    }

    #[test]
    fn modifier_counts_match_the_xml_op_sequence() {
        let cube = cube();
        assert_eq!((cube.positions.len(), cube.face_count()), (8, 12));
        let sub2 = subdivide(
            &cube,
            SubdivideParams {
                num_steps: 1,
                smoothness: 1.0,
            },
        );
        assert_eq!((sub2.positions.len(), sub2.face_count()), (26, 48));
        let selected = select_faces(
            &sub2,
            SelectParams {
                selection_type: 4,
                logic: 1,
                invert: false,
                output: 1,
                random_factor: 1.0,
                random_seed: 1,
            },
        );
        assert_eq!((selected.positions.len(), selected.face_count()), (26, 48));
        let extruded = extrude(
            &selected,
            ExtrudeParams {
                num_steps: 2,
                amount: 0.88,
                direction: 0,
                local_scale: 0.0,
            },
        );
        assert_eq!(
            (extruded.positions.len(), extruded.face_count()),
            (336, 480)
        );
        let jittered = randomize(
            &extruded,
            RandomizeParams {
                amount: 0.1,
                direction: 0,
                random_seed: 1,
            },
        );
        assert_eq!(
            (jittered.positions.len(), jittered.face_count()),
            (336, 480)
        );
        let sub1 = subdivide(
            &jittered,
            SubdivideParams {
                num_steps: 2,
                smoothness: 1.0,
            },
        );
        assert_eq!((sub1.positions.len(), sub1.face_count()), (3936, 7680));
    }

    #[test]
    fn extrusion_collapses_the_cap_to_an_apex_at_the_requested_amount() {
        // A single +z face with local_scale 0 and two steps: a cone whose apex
        // sits `amount` above the face plane.
        let face = IndexedMesh {
            positions: vec![[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.0, 0.5, 0.5]],
            triangles: vec![[0, 1, 2]],
        };
        let cone = extrude(
            &face,
            ExtrudeParams {
                num_steps: 2,
                amount: 0.88,
                direction: 0,
                local_scale: 0.0,
            },
        );
        let apexes = cone
            .positions
            .iter()
            .filter(|position| (position[2] - (0.5 + 0.88)).abs() < 1e-5)
            .count();
        assert_eq!(apexes, 1, "one collapsed apex per face");
        // Base + one intermediate ring + apex.
        assert_eq!(cone.positions.len(), 3 + 3 + 1);
    }

    #[test]
    fn local_scale_zero_does_not_shrink_the_base_ring() {
        let face = IndexedMesh {
            positions: vec![[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.0, 0.5, 0.5]],
            triangles: vec![[0, 1, 2]],
        };
        let cone = extrude(
            &face,
            ExtrudeParams {
                num_steps: 2,
                amount: 0.5,
                direction: 0,
                local_scale: 0.0,
            },
        );
        // The intermediate ring is halfway between the base corner and the
        // centroid, not collapsed.
        let midway = cone.positions[3];
        assert!((midway[2] - 0.75).abs() < 1e-5, "{midway:?}");
        assert!((length([midway[0], midway[1], 0.0])).abs() > 0.1);
    }

    #[test]
    fn randomize_is_deterministic_and_bounded() {
        let mesh = cube();
        let params = RandomizeParams {
            amount: 0.1,
            direction: 0,
            random_seed: 1,
        };
        let first = randomize(&mesh, params);
        let second = randomize(&mesh, params);
        assert_eq!(first, second);
        for (before, after) in mesh.positions.iter().zip(&first.positions) {
            let displacement = length(sub(*after, *before));
            assert!(displacement <= 0.1 + 1e-5, "moved {displacement}");
        }
    }

    #[test]
    fn the_full_chain_keeps_the_tip_mask_band() {
        let mesh = crate::point10::scenes::spiky::build_spiky_mesh();
        let (min, max) = mesh.radius_range();
        // `Randomize1`'s signed +/-0.1 jitter can pull an inner corner below
        // the pristine 0.5 cube face, so the lower bound is the vendor bound
        // rather than the pre-jitter one.
        assert!(
            (0.45..0.6).contains(&min),
            "inner radius {min} left the jitter band"
        );
        assert!(
            max > 1.175,
            "max radius {max} never reaches the red tip mask"
        );
        assert!(max < 1.35, "max radius {max} floods the tip mask");
        // Every spike is a closed-ish cone: no triangle collapsed.
        assert!(mesh.triangles.iter().all(|triangle| {
            let [a, b, c] = triangle.map(|index| mesh.positions[index as usize]);
            length(cross(sub(b, a), sub(c, a))) > 1e-8
        }));
        let mut referenced = vec![false; mesh.positions.len()];
        for triangle in &mesh.triangles {
            for index in triangle {
                referenced[*index as usize] = true;
            }
        }
        let unreferenced = mesh
            .positions
            .iter()
            .enumerate()
            .filter(|(index, position)| {
                !referenced[*index] && position[0].hypot(position[1]).hypot(position[2]) > 1.175
            })
            .count();
        assert_eq!(unreferenced, 0, "tip vertices must be indexed by triangles");
        let tip_triangles = mesh
            .triangles
            .iter()
            .filter(|triangle| {
                triangle.iter().all(|index| {
                    let p = mesh.positions[*index as usize];
                    p[0].hypot(p[1]).hypot(p[2]) > 1.0
                })
            })
            .count();
        assert!(tip_triangles > 0);
        assert_eq!(mesh.face_count(), 48 * 10 * 4 * 4);
    }
}
