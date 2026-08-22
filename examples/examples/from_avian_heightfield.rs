//! Demonstrates navmesh generation from Avian3D heightfield colliders.
//!
//! Press SPACE to generate the navmesh.

use avian_rerecast::prelude::*;
use avian3d::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
    color::palettes::tailwind,
    input::common_conditions::input_just_pressed,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    remote::{http::RemoteHttpPlugin, RemotePlugin},
};
use bevy_rerecast::{debug::DetailNavmeshGizmo, prelude::*};

fn main() -> AppExit {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: "../assets".to_string(),
            ..default()
        }))
        .add_plugins(PhysicsPlugins::default())
        .add_plugins((RemotePlugin::default(), RemoteHttpPlugin::default()))
        .add_plugins((NavmeshPlugins::default(), AvianBackendPlugin::default()))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            generate_navmesh.run_if(input_just_pressed(KeyCode::Space)),
        )
        .add_observer(configure_camera)
        .run()
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Heightfield parameters
    let resolution = 64;
    let world_size = 50.0;
    let height_scale = 5.0;

    // Generate procedural heightfield using sine waves
    let heights = generate_heights(resolution, 8.0);
    let scale = Vec3::new(world_size, height_scale, world_size);

    // Create visualization mesh
    let mesh = heightfield_to_mesh(&heights, scale);

    // Spawn the heightfield terrain
commands.spawn((
        Name::new("Terrain Child test"),
        Visibility::default(),
        Transform::default(),
        children![(
                Name::new("Child"),
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::from(tailwind::GREEN_600),
                    perceptual_roughness: 0.9,
                    ..default()
                })),
                RigidBody::Static,
                Collider::heightfield(heights, scale.into()),
        )]
    ));

    

    // Add some obstacles on the terrain
    let obstacle_material = materials.add(Color::from(tailwind::GRAY_400));
    let shape = Cuboid::new(3.0, 4.0, 3.0);
    commands.spawn((
        Name::new("Obstacle 1"),
        Mesh3d(meshes.add(shape)),
        MeshMaterial3d(obstacle_material.clone()),
        RigidBody::Static,
        Collider::from(shape),
        Transform::from_xyz(-8.0, 5.0, -8.0),
    ));

    let shape = Cuboid::new(5.0, 2.0, 2.0);
    commands.spawn((
        Name::new("Obstacle 2"),
        Mesh3d(meshes.add(shape)),
        MeshMaterial3d(obstacle_material.clone()),
        RigidBody::Static,
        Collider::from(shape),
        Transform::from_xyz(10.0, 4.0, 5.0),
    ));

    // Lighting
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            illuminance: 15000.0,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(0.5, -1.0, 0.3), Vec3::Y),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(40.0, 30.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Instructions
    commands.spawn((
        Text::new("Press SPACE to generate navmesh"),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
    ));
}

/// Generate procedural heights using sine waves.
fn generate_heights(resolution: usize, frequency: f32) -> Vec<Vec<f32>> {
    let mut heights = Vec::with_capacity(resolution);
    for x in 0..resolution {
        let mut row = Vec::with_capacity(resolution);
        for z in 0..resolution {
            let fx = x as f32 / resolution as f32;
            let fz = z as f32 / resolution as f32;

            // Combine multiple sine waves for interesting terrain
            let h1 = (fx * frequency).sin() * (fz * frequency).cos();
            let h2 = (fx * frequency * 2.0 + 1.0).sin() * 0.3;
            let h3 = (fz * frequency * 1.5).cos() * 0.2;

            row.push(h1 + h2 + h3);
        }
        heights.push(row);
    }
    heights
}

/// Convert heightfield data to a renderable mesh.
/// Note: Avian's heightfield uses nalgebra DMatrix which is column-major,
/// so we need to match that layout when generating the visual mesh.
fn heightfield_to_mesh(heights: &[Vec<f32>], scale: Vec3) -> Mesh {
    let num_x = heights.len();
    let num_z = heights[0].len();

    let mut positions = Vec::with_capacity(num_x * num_z);
    let mut normals = Vec::with_capacity(num_x * num_z);
    let mut uvs = Vec::with_capacity(num_x * num_z);

    // Generate vertices - iterate in same order as avian's DMatrix storage
    for z in 0..num_z {
        for x in 0..num_x {
            let fx = x as f32 / (num_x - 1) as f32 - 0.5;
            let fz = z as f32 / (num_z - 1) as f32 - 0.5;
            let height = heights[x][z];

            positions.push([fx * scale.x, height * scale.y, fz * scale.z]);
            uvs.push([fx + 0.5, fz + 0.5]);
        }
    }

    // Calculate normals
    for z in 0..num_z {
        for x in 0..num_x {
            let get_height = |x: i32, z: i32| -> f32 {
                let x = x.clamp(0, num_x as i32 - 1) as usize;
                let z = z.clamp(0, num_z as i32 - 1) as usize;
                heights[x][z] * scale.y
            };

            let hx = x as i32;
            let hz = z as i32;

            let dx = get_height(hx + 1, hz) - get_height(hx - 1, hz);
            let dz = get_height(hx, hz + 1) - get_height(hx, hz - 1);

            let step_x = scale.x / (num_x - 1) as f32;
            let step_z = scale.z / (num_z - 1) as f32;

            let normal = Vec3::new(-dx / (2.0 * step_x), 1.0, -dz / (2.0 * step_z)).normalize();
            normals.push([normal.x, normal.y, normal.z]);
        }
    }

    // Generate indices - matching vertex iteration order (z outer, x inner)
    let mut indices = Vec::with_capacity((num_x - 1) * (num_z - 1) * 6);
    for z in 0..(num_z - 1) {
        for x in 0..(num_x - 1) {
            let i00 = (z * num_x + x) as u32;
            let i10 = (z * num_x + x + 1) as u32;
            let i01 = ((z + 1) * num_x + x) as u32;
            let i11 = ((z + 1) * num_x + x + 1) as u32;

            // Two triangles per quad - CCW winding for top-facing
            indices.extend_from_slice(&[i00, i01, i11]);
            indices.extend_from_slice(&[i00, i11, i10]);
        }
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

#[derive(Resource)]
#[allow(dead_code)]
struct NavmeshHandle(Handle<Navmesh>);

fn generate_navmesh(mut generator: NavmeshGenerator, mut commands: Commands) {
    let settings = NavmeshSettings {
        walkable_slope_angle: 45.0_f32.to_radians(),
        ..default()
    };
    let navmesh = generator.generate(settings);
    commands.spawn(DetailNavmeshGizmo::new(&navmesh));
    commands.insert_resource(NavmeshHandle(navmesh));
}

fn configure_camera(
    trigger: On<Add<Camera>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    commands.entity(trigger.entity).insert(EnvironmentMapLight {
        diffuse_map: asset_server.load("environment_maps/voortrekker_interior_1k_diffuse.ktx2"),
        specular_map: asset_server.load("environment_maps/voortrekker_interior_1k_specular.ktx2"),
        intensity: 2000.0,
        ..default()
    });
}
