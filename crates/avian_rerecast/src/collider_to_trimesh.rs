//! Contains traits and methods for converting [`Collider`]s into trimeshes, expressed as [`TrimeshedCollider`]s.

use avian3d::{
    math::{ToF32Precision, ToRealPrecision},
    parry::{
        bounding_volume::Aabb,
        math::Vector,
        shape::{Compound, HeightField, Triangle, TypedShape},
    },
    prelude::*,
};
use bevy_math::prelude::*;
use bevy_rerecast_core::rerecast::{AreaType, TriMesh};
use bevy_shape::Aabb3d;

/// Convenience trait that allows a [`Collider`] to be converted into a [`TriMesh`].
pub trait ColliderToTriMesh {
    /// Converts the collider into a [`TriMesh`].
    ///
    /// # Arguments
    ///
    /// * `subdivisions` - The number of subdivisions to use for the collider. This is used for curved shapes such as circles and spheres.
    ///
    /// # Returns
    ///
    /// A [`TriMesh`] if the collider is supported, otherwise `None`
    ///
    /// The following shapes are not supported:
    /// - [`Segment`](avian3d::parry::shape::Segment)
    /// - [`Polyline`](avian3d::parry::shape::Polyline)
    /// - [`HalfSpace`](avian3d::parry::shape::HalfSpace)
    /// - Custom shapes
    ///
    /// The following rounded shapes are supported, but only the inner shape without rounding is used:
    /// - [`RoundCuboid`](avian3d::parry::shape::RoundCuboid)
    /// - [`RoundTriangle`](avian3d::parry::shape::RoundTriangle)
    /// - [`RoundConvexPolyhedron`](avian3d::parry::shape::RoundConvexPolyhedron)
    /// - [`RoundCylinder`](avian3d::parry::shape::RoundCylinder)
    /// - [`RoundCone`](avian3d::parry::shape::RoundCone)
    fn to_trimesh(
        &self,
        pos: impl Into<Position>,
        rot: impl Into<Rotation>,
        subdivisions: u32,
    ) -> Option<TriMesh>;

    /// Like [`Self::to_trimesh`], but heightfields and trimeshes only emit the triangles that
    /// may touch `clip` (world space), and heightfields sample every `heightfield_stride`-th
    /// row and column. Other shapes are converted whole.
    fn to_trimesh_clipped(
        &self,
        pos: impl Into<Position>,
        rot: impl Into<Rotation>,
        subdivisions: u32,
        clip: Option<&Aabb3d>,
        heightfield_stride: usize,
    ) -> Option<TriMesh>;
}

impl ColliderToTriMesh for Collider {
    fn to_trimesh(
        &self,
        pos: impl Into<Position>,
        rot: impl Into<Rotation>,
        subdivisions: u32,
    ) -> Option<TriMesh> {
        self.to_trimesh_clipped(pos, rot, subdivisions, None, 1)
    }

    fn to_trimesh_clipped(
        &self,
        pos: impl Into<Position>,
        rot: impl Into<Rotation>,
        subdivisions: u32,
        clip: Option<&Aabb3d>,
        heightfield_stride: usize,
    ) -> Option<TriMesh> {
        shape_to_trimesh(
            &self.shape_scaled().as_typed_shape(),
            pos.into(),
            rot.into(),
            subdivisions,
            clip,
            heightfield_stride,
        )
    }
}

/// `clip` in the shape's local frame: the box around its eight corners.
fn local_aabb(clip: &Aabb3d, pos: Position, rot: Rotation) -> Aabb {
    let pos = Vec3::from(pos.f32());
    let inv = rot.0.f32().inverse();
    let (min, max) = (Vec3::from(clip.min), Vec3::from(clip.max));
    let (mut mins, mut maxs) = (Vec3::MAX, Vec3::MIN);
    for i in 0..8 {
        let corner = Vec3::select(BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0), max, min);
        let local = inv * (corner - pos);
        mins = mins.min(local);
        maxs = maxs.max(local);
    }
    Aabb::new(mins.real(), maxs.real())
}

/// The heightfield's triangles on a lattice of every `stride`-th vertex, aligned to multiples of
/// `stride` so neighbouring bakes (and neighbouring heightfields of the same stride) agree.
fn strided_heightfield(
    height_field: &HeightField,
    clip: Option<&Aabb>,
    stride: usize,
) -> Vec<Triangle> {
    let (rows, cols) = (height_field.nrows(), height_field.ncols());
    let (z, x) = match clip {
        Some(aabb) => {
            let (z, x) = height_field.unclamped_elements_range_in_local_aabb(aabb);
            let clamp = |r: core::ops::Range<isize>, n: usize| {
                r.start.clamp(0, n as isize) as usize..r.end.clamp(0, n as isize) as usize
            };
            (clamp(z, rows), clamp(x, cols))
        }
        None => (0..rows, 0..cols),
    };
    let heights = height_field.heights();
    let y_scale = height_field.scale().y;
    let at = |i: usize, j: usize| {
        Vector::new(
            height_field.x_at(j),
            heights[(i, j)] * y_scale,
            height_field.z_at(i),
        )
    };
    let mut triangles = Vec::new();
    for i in (z.start / stride * stride..z.end).step_by(stride) {
        let i1 = (i + stride).min(rows);
        for j in (x.start / stride * stride..x.end).step_by(stride) {
            let j1 = (j + stride).min(cols);
            let (p00, p10, p01, p11) = (at(i, j), at(i1, j), at(i, j1), at(i1, j1));
            triangles.push(Triangle::new(p00, p10, p01));
            triangles.push(Triangle::new(p10, p11, p01));
        }
    }
    triangles
}

fn triangle_soup(triangles: Vec<Triangle>) -> (Vec<Vector>, Vec<[u32; 3]>) {
    let indices = (0..triangles.len() as u32)
        .map(|i| [i * 3, i * 3 + 1, i * 3 + 2])
        .collect();
    let vertices = triangles
        .into_iter()
        .flat_map(|t| [t.a, t.b, t.c])
        .collect();
    (vertices, indices)
}

fn shape_to_trimesh(
    shape: &TypedShape,
    pos: Position,
    rot: Rotation,
    subdivisions: u32,
    clip: Option<&Aabb3d>,
    heightfield_stride: usize,
) -> Option<TriMesh> {
    let local_clip = clip.map(|clip| local_aabb(clip, pos, rot));
    let (vertices, indices) = match shape {
        TypedShape::HeightField(height_field) if heightfield_stride > 1 => triangle_soup(
            strided_heightfield(height_field, local_clip.as_ref(), heightfield_stride),
        ),
        TypedShape::HeightField(height_field) if let Some(aabb) = &local_clip => {
            let mut triangles = Vec::new();
            height_field.map_elements_in_local_aabb(aabb, &mut |_, t| triangles.push(*t));
            triangle_soup(triangles)
        }
        TypedShape::TriMesh(tri_mesh) if let Some(aabb) = &local_clip => triangle_soup(
            tri_mesh
                .bvh()
                .intersect_aabb(aabb)
                .map(|i| tri_mesh.triangle(i))
                .collect(),
        ),
        // Simple cases
        TypedShape::Cuboid(cuboid) => cuboid.to_trimesh(),
        TypedShape::Voxels(voxels) => voxels.to_trimesh(),
        TypedShape::ConvexPolyhedron(convex_polyhedron) => convex_polyhedron.to_trimesh(),
        TypedShape::HeightField(height_field) => height_field.to_trimesh(),
        // Triangles
        TypedShape::Triangle(triangle) => {
            (vec![triangle.a, triangle.b, triangle.c], vec![[0, 1, 2]])
        }
        TypedShape::TriMesh(tri_mesh) => {
            (tri_mesh.vertices().to_vec(), tri_mesh.indices().to_vec())
        }
        // Need subdivisions
        TypedShape::Ball(ball) => ball.to_trimesh(subdivisions, subdivisions),
        TypedShape::Capsule(capsule) => capsule.to_trimesh(subdivisions, subdivisions),
        TypedShape::Cylinder(cylinder) => cylinder.to_trimesh(subdivisions),
        TypedShape::Cone(cone) => cone.to_trimesh(subdivisions),
        // Compounds need to be unpacked
        TypedShape::Compound(compound) => {
            return Some(compound_trimesh(
                compound,
                pos,
                rot,
                subdivisions,
                clip,
                heightfield_stride,
            ));
        }
        // Rounded shapes ignore the rounding and use the inner shape
        TypedShape::RoundCuboid(round_shape) => round_shape.inner_shape.to_trimesh(),
        TypedShape::RoundTriangle(round_shape) => (
            vec![
                round_shape.inner_shape.a,
                round_shape.inner_shape.b,
                round_shape.inner_shape.c,
            ],
            vec![[0, 1, 2]],
        ),
        TypedShape::RoundConvexPolyhedron(round_shape) => round_shape.inner_shape.to_trimesh(),
        TypedShape::RoundCylinder(round_shape) => round_shape.inner_shape.to_trimesh(subdivisions),
        TypedShape::RoundCone(round_shape) => round_shape.inner_shape.to_trimesh(subdivisions),
        // Not supported
        TypedShape::Segment(_segment) => return None,
        TypedShape::Polyline(_polyline) => return None,
        TypedShape::HalfSpace(_half_space) => return None,
        TypedShape::Custom(_shape) => return None,
    };
    let indices_len = indices.len();
    let pos = Vec3A::from(pos.f32());
    Some(TriMesh {
        vertices: vertices
            .into_iter()
            .map(|v| pos + Vec3A::from((rot * v).f32()))
            .collect(),
        indices: indices.into_iter().map(|i| i.into()).collect(),
        area_types: vec![AreaType::NOT_WALKABLE; indices_len],
    })
}

fn compound_trimesh(
    compound: &Compound,
    pos: Position,
    rot: Rotation,
    subdivisions: u32,
    clip: Option<&Aabb3d>,
    heightfield_stride: usize,
) -> TriMesh {
    compound.shapes().iter().fold(
        TriMesh::default(),
        |mut compound_trimesh, (sub_pos, shape)| {
            let pos = Position(pos.0 + rot * sub_pos.translation);
            let rot = Rotation((rot.mul_quat(sub_pos.rotation)).normalize());
            let Some(trimesh) =
                // No need to track recursive compounds because parry panics on nested compounds anyways lol
                shape_to_trimesh(&shape.as_typed_shape(), pos, rot, subdivisions, clip, heightfield_stride)
            else {
                return compound_trimesh;
            };

            compound_trimesh.extend(trimesh);
            compound_trimesh
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_cuboid() {
        let collider = Collider::cuboid(1.0, 2.0, 3.0);
        let trimesh = collider
            .to_trimesh(Position::default(), Rotation::default(), 1)
            .unwrap();
        assert_eq!(trimesh.vertices.len(), 8);
        assert_eq!(trimesh.indices.len(), 12);
    }
}
