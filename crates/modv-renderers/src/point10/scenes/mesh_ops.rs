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
//! | `Extrude` | `NumSteps`, `Amount`, `Direction`, `Center`, `LocalScale` | [`extrude`] |
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
    /// `Center` — an offset added to every extrusion direction/anchor.
    pub center: [f32; 3],
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

/// `Extrude`: raise a tapered spike from every face.
///
/// `LocalScale 0` collapses the cap to a single apex, turning each face into a
/// cone (Spiky's spike); a non-zero value keeps a smaller copy of the face at
/// the cap and closes it with a triangle. `NumSteps 2` emits one intermediate
/// ring before the apex.
///
/// The input base vertices are reused in place and the base triangle is **not**
/// emitted. That is what makes the result one watertight body: adjacent spikes
/// meet along their shared base edge, and each such edge is used by exactly the
/// two wall triangles on either side. Retaining the base triangle as well would
/// give every original edge four incident triangles (two bases plus two walls,
/// two closed shells joined along an interior membrane), which is precisely
/// how the previous implementation produced 48 disconnected hollow shards: the
/// retained bases became internal walls, so the star had no solid centre.
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
    let center = if params.center.iter().all(|value| value.is_finite()) {
        params.center
    } else {
        [0.0; 3]
    };

    // Reuse the input positions so neighbouring faces share their base ring.
    let mut positions = mesh.positions.clone();
    let mut triangles = Vec::new();

    for face in &mesh.triangles {
        let base = *face;
        let corners = base.map(|index| mesh.positions[index as usize]);
        let centroid = scale(add(add(corners[0], corners[1]), corners[2]), 1.0 / 3.0);
        let normal = normalise(cross(
            sub(corners[1], corners[0]),
            sub(corners[2], corners[0]),
        ));

        // Direction 0 is the face normal; other directions are not used by
        // Spiky and fall back to the same normal.
        let direction = normal;
        let anchor = add(centroid, center);

        // rings[n][corner] vertex indices; ring 0 is the shared base face.
        let mut rings: Vec<[u32; 3]> = Vec::with_capacity(steps as usize + 1);
        rings.push(base);
        let mut apex: Option<u32> = None;

        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let cap_scale = 1.0 + (local_scale - 1.0) * t;
            if step == steps && local_scale == 0.0 {
                // All three cap vertices land on the same point: emit one apex.
                let apex_position = add(anchor, scale(direction, amount * t));
                let index = positions.len() as u32;
                positions.push(apex_position);
                apex = Some(index);
                break;
            }
            let mut ring = [0_u32; 3];
            for (corner, vertex) in corners.iter().enumerate() {
                let tangential = sub(*vertex, centroid);
                let ring_position = add(
                    add(anchor, scale(direction, amount * t)),
                    scale(tangential, cap_scale),
                );
                ring[corner] = positions.len() as u32;
                positions.push(ring_position);
            }
            rings.push(ring);
        }

        for pair in rings.windows(2) {
            let lower = pair[0];
            let upper = pair[1];
            for edge in 0..3 {
                let next = (edge + 1) % 3;
                triangles.push([lower[edge], lower[next], upper[next]]);
                triangles.push([lower[edge], upper[next], upper[edge]]);
            }
        }

        if let Some(apex) = apex {
            // Connect the last full ring to the collapsed apex.
            let ring = *rings.last().expect("a base ring exists");
            for edge in 0..3 {
                let next = (edge + 1) % 3;
                triangles.push([ring[edge], ring[next], apex]);
            }
        } else {
            // A non-zero `LocalScale` leaves a smaller face at the cap; close
            // it with the same winding as the input face.
            triangles.push(*rings.last().expect("a base ring exists"));
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
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    fn triangle_key(mesh: &IndexedMesh, triangle: [u32; 3]) -> [[i64; 3]; 3] {
        let mut corners: Vec<_> = triangle
            .map(|index| quantise(mesh.positions[index as usize]))
            .to_vec();
        corners.sort_unstable();
        [corners[0], corners[1], corners[2]]
    }

    fn face_keys(mesh: &IndexedMesh) -> BTreeSet<[[i64; 3]; 3]> {
        mesh.triangles
            .iter()
            .map(|&triangle| triangle_key(mesh, triangle))
            .collect()
    }

    fn cube() -> IndexedMesh {
        from_box(1.0, 1.0, 1.0)
    }

    /// Connected components after welding triangle connectivity by vertex index.
    ///
    /// This deliberately uses the emitted indices, not coincident positions:
    /// private base copies are the defect this contract needs to expose.
    fn connected_components_by_index(mesh: &IndexedMesh) -> usize {
        fn find(parent: &mut [usize], mut node: usize) -> usize {
            while parent[node] != node {
                parent[node] = parent[parent[node]];
                node = parent[node];
            }
            node
        }

        fn union(parent: &mut [usize], a: usize, b: usize) {
            let a = find(parent, a);
            let b = find(parent, b);
            if a != b {
                parent[b] = a;
            }
        }

        let mut parent: Vec<_> = (0..mesh.triangles.len()).collect();
        let mut first_triangle = vec![None; mesh.positions.len()];
        for (face, triangle) in mesh.triangles.iter().enumerate() {
            for &vertex in triangle {
                let vertex = vertex as usize;
                if let Some(first) = first_triangle[vertex] {
                    union(&mut parent, face, first);
                } else {
                    first_triangle[vertex] = Some(face);
                }
            }
        }
        (0..parent.len())
            .map(|face| find(&mut parent, face))
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// Histogram of undirected-edge incidence after welding coincident positions.
    ///
    /// Index-only incidence would call each private cone shell manifold even
    /// though coincident base seams belong to four triangles geometrically.
    fn position_welded_edge_incidence(mesh: &IndexedMesh) -> BTreeMap<usize, usize> {
        let mut vertex_ids = HashMap::new();
        let mut next_id = 0_u32;
        let remap: Vec<u32> = mesh
            .positions
            .iter()
            .map(|&position| {
                *vertex_ids.entry(quantise(position)).or_insert_with(|| {
                    let id = next_id;
                    next_id += 1;
                    id
                })
            })
            .collect();

        let mut edges = BTreeMap::new();
        for triangle in &mesh.triangles {
            let triangle = triangle.map(|vertex| remap[vertex as usize]);
            for (a, b) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                *edges.entry(ordered_edge(a, b)).or_insert(0_usize) += 1;
            }
        }

        let mut histogram = BTreeMap::new();
        for uses in edges.into_values() {
            *histogram.entry(uses).or_insert(0_usize) += 1;
        }
        histogram
    }

    fn triangle_area(mesh: &IndexedMesh, triangle: [u32; 3]) -> f32 {
        let [a, b, c] = triangle.map(|index| mesh.positions[index as usize]);
        length(cross(sub(b, a), sub(c, a))) * 0.5
    }

    /// Number of intersections for a ray from `origin` along +Z.
    ///
    /// This is Möller–Trumbore without the intersection point: an odd count is
    /// the usual structural inside/outside test for a closed triangle surface.
    fn positive_z_crossings(mesh: &IndexedMesh, origin: [f32; 3]) -> usize {
        let direction = [0.0_f32, 0.0, 1.0];
        let mut crossings = 0;
        for triangle in &mesh.triangles {
            let [a, b, c] = triangle.map(|index| mesh.positions[index as usize]);
            let edge1 = sub(b, a);
            let edge2 = sub(c, a);
            let h = cross(direction, edge2);
            let determinant = dot(edge1, h);
            if determinant.abs() < 1e-6 {
                continue;
            }
            let inverse = 1.0 / determinant;
            let s = sub(origin, a);
            let u = dot(s, h) * inverse;
            if !(-1e-6..=1.0 + 1e-6).contains(&u) {
                continue;
            }
            let q = cross(s, edge1);
            let v = dot(direction, q) * inverse;
            if v < -1e-6 || u + v > 1.0 + 1e-6 {
                continue;
            }
            let distance = dot(edge2, q) * inverse;
            if distance > 1e-6 {
                crossings += 1;
            }
        }
        crossings
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
                center: [0.0; 3],
                local_scale: 0.0,
            },
        );
        assert!(extruded.face_count() > selected.face_count());
        let jittered = randomize(
            &extruded,
            RandomizeParams {
                amount: 0.1,
                direction: 0,
                random_seed: 1,
            },
        );
        assert_eq!(jittered.face_count(), extruded.face_count());
        let sub1 = subdivide(
            &jittered,
            SubdivideParams {
                num_steps: 2,
                smoothness: 1.0,
            },
        );
        assert_eq!(sub1.face_count(), extruded.face_count() * 16);
        assert!(sub1.positions.len() > jittered.positions.len());
    }

    /// Spiky's extrusion must consume each selected base triangle and replace it
    /// with walls through one intermediate ring to an apex, leaving one closed
    /// genus-0 surface. Retaining a per-face base is incompatible with that
    /// contract: two base triangles plus two wall triangles would meet on every
    /// original edge, creating a four-face interior membrane rather than a
    /// two-manifold solid.
    #[test]
    fn spiky_extrusion_preserves_one_manifold_solid_body() {
        // `Things/Spiky.xml`: Cube1 -> Subdivide2 -> SelectMesh1 -> Extrude2
        // -> Randomize1 -> Subdivide1. Extrude2's ports are NumSteps 2,
        // Amount 0.88, Direction 0, Center 0 0 0, LocalScale 0.
        let params = ExtrudeParams {
            num_steps: 2,
            amount: 0.88,
            direction: 0,
            center: [0.0; 3],
            local_scale: 0.0,
        };
        let subdivided_cube = subdivide(
            &cube(),
            SubdivideParams {
                num_steps: 1,
                smoothness: 1.0,
            },
        );
        let selected = select_faces(
            &subdivided_cube,
            SelectParams {
                selection_type: 4,
                logic: 1,
                invert: false,
                output: 1,
                random_factor: 1.0,
                random_seed: 1,
            },
        );
        let extruded = extrude(&selected, params);

        let components = connected_components_by_index(&extruded);
        let incidence = position_welded_edge_incidence(&extruded);
        let boundary_edges = incidence.get(&1).copied().unwrap_or(0);
        let non_manifold_edges = incidence
            .iter()
            .filter_map(|(&uses, &count)| (uses != 2).then_some(count))
            .sum::<usize>();

        let jittered = randomize(
            &extruded,
            RandomizeParams {
                amount: 0.1,
                direction: 0,
                random_seed: 1,
            },
        );
        let smoothed = subdivide(
            &jittered,
            SubdivideParams {
                num_steps: 2,
                smoothness: 1.0,
            },
        );

        // Referenced, position-welded topology: V=218, F=432 and E=3F/2=648 give χ=2,
        // the signature of one closed genus-0 surface.
        let welded_vertex_count = extruded
            .triangles
            .iter()
            .flatten()
            .map(|&vertex| quantise(extruded.positions[vertex as usize]))
            .collect::<BTreeSet<_>>()
            .len();
        let welded_edge_count = incidence.values().sum::<usize>();
        let euler_characteristic =
            welded_vertex_count as i64 - welded_edge_count as i64 + extruded.face_count() as i64;

        // The solid must retain real surface around its body, not just distant
        // spike tips. Measure the full-chain result that previously lost 69% of
        // its area when the cloned base shells collapsed.
        let mut total_area = 0.0_f32;
        let mut centre_area = 0.0_f32;
        for &triangle in &smoothed.triangles {
            let area = triangle_area(&smoothed, triangle);
            total_area += area;
            let [a, b, c] = triangle.map(|index| smoothed.positions[index as usize]);
            let centroid = scale(add(add(a, b), c), 1.0 / 3.0);
            if length(centroid) < 0.8 {
                centre_area += area;
            }
        }
        let centre_area_fraction = centre_area / total_area;

        // Structural through-hole check: probe the cube's central Z=0
        // cross-section well inside its 0.5 inscribed radius. A solid body has an
        // odd number of +Z surface crossings at every probe; the hollow cage has
        // an even count. Asymmetric probes avoid exact vertices and shared edges,
        // and no rasterisation or visual threshold is involved.
        let centre_samples = [
            [0.0137, -0.0211, 0.0],
            [0.1370, 0.0910, 0.0],
            [-0.1730, 0.1170, 0.0],
            [0.0710, -0.1810, 0.0],
            [-0.1310, -0.1490, 0.0],
        ];
        let outside_centre_samples: Vec<_> = centre_samples
            .into_iter()
            .map(|point| {
                let crossings = positive_z_crossings(&smoothed, point);
                (point, crossings)
            })
            .filter(|(_, crossings)| crossings % 2 == 0)
            .collect();

        let mut failures = Vec::new();
        if (
            params.num_steps,
            params.amount,
            params.direction,
            params.local_scale,
        ) != (2, 0.88_f32, 0, 0.0)
        {
            failures.push(format!(
                "Extrude2 parameters: expected (steps=2, amount=0.88, direction=0, local_scale=0), got {:?}",
                (params.num_steps, params.amount, params.direction, params.local_scale)
            ));
        }
        if components != 1 {
            failures.push(format!(
                "connected components: expected 1, got {components}"
            ));
        }
        if non_manifold_edges != 0 {
            failures.push(format!(
                "watertight/manifold edges: expected every position-welded undirected edge to have 2 incident triangles, got {non_manifold_edges} bad edges ({boundary_edges} boundary; incidence histogram {incidence:?})"
            ));
        }
        // 26 shared input vertices + 48 × (3 intermediate-ring vertices + 1 apex).
        if extruded.positions.len() != 26 + 48 * 4 {
            failures.push(format!(
                "extruded vertex count: expected {}, got {}",
                26 + 48 * 4,
                extruded.positions.len()
            ));
        }
        // 48 × (3 wall quads × 2 triangles + 3 apex-fan triangles).
        if extruded.face_count() != 48 * 9 {
            failures.push(format!(
                "extruded face count: expected {}, got {}",
                48 * 9,
                extruded.face_count()
            ));
        }
        if euler_characteristic != 2 {
            failures.push(format!(
                "Euler characteristic: expected χ=2 for one closed genus-0 surface, got χ={euler_characteristic} (V={welded_vertex_count}, E={welded_edge_count}, F={})",
                extruded.face_count()
            ));
        }
        // Measured centre coverage is 61.55% (9.489177 / 15.416608). Keep a
        // twelve-point margin while still rejecting the previous 69% collapse,
        // which left only 31% of the surface near the body.
        if !centre_area_fraction.is_finite() || centre_area_fraction < 0.50 {
            failures.push(format!(
                "centre surface area after Subdivide1: expected at least 50.0% of {total_area:.6}, got {centre_area:.6} ({:.1}%)",
                centre_area_fraction * 100.0
            ));
        }
        if !outside_centre_samples.is_empty() {
            failures.push(format!(
                "solid centre: expected odd +Z crossing counts on the central cross-section, outside probes {outside_centre_samples:?}"
            ));
        }

        assert!(
            failures.is_empty(),
            "Spiky extrusion topology invariants failed:\n{}",
            failures.join("\n")
        );
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
                center: [0.0; 3],
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
                center: [0.0; 3],
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
        // Two subdivision passes quadruple the triangle count, so the 48
        // extruded faces (nine spike triangles each) end at 48 * 9 * 16.
        assert_eq!(mesh.face_count(), 48 * 9 * 16);
    }
}
