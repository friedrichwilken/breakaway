//! Pseudo-3D presentation layer, after after-zelda's `pseudo3d.rs`: the
//! logical 2D plane (x = east, y = north) is mirrored into a 3D scene at
//! render (x, height, -y). A chase `Camera3d` looks down the road; riders
//! are upright quads turned to face the camera, so perspective scales them.
//! All positions come from the terrain `Profile`: the centre line bends in
//! the plane (`centre_at` / `right_at`) and its heights are exaggerated for
//! drawing only (`render_height`). Road, markings and shoulder are mesh
//! strips rebuilt from cross-sections every 3 m; beyond them a flat base
//! plane and a backdrop ring (far land + mountains) fill every direction.
//! The partner's place and the camera distance follow the scenario layer's
//! `Staging`, eased; once a breakaway is caught a bunch of riders rides
//! around the player.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

use crate::scenario::{BunchView, CAMERA_BEHIND_M, PULL_CAMERA_BEHIND_M, PULL_PARTNER_LATERAL_M, Scenario, Staging};
use crate::sim::{ROAD_HALF_WIDTH_M, RaceState};
use crate::terrain::Profile;

// ── Camera ────────────────────────────────────────────────────────────────────
/// The camera's distance behind the rider comes from the staging (6 m, 9 m
/// on a pull); recycled items start behind the farthest.
const CAM_BEHIND_MAX_M: f32 = PULL_CAMERA_BEHIND_M;
const CAM_HEIGHT_M: f32 = 3.0;
const CAM_LOOK_AHEAD_M: f32 = 10.0;
/// Share of the road's rise between eye and look point that the camera's
/// pitch follows; the rest shows on screen as the road rising or falling.
const CAM_SLOPE_FOLLOW: f32 = 0.5;
/// Far clip plane; must reach past the backdrop.
const CAM_FAR_M: f32 = 8000.0;

// ── Terrain ───────────────────────────────────────────────────────────────────
/// Drawn heights are this many times the true ones (render only; sim, HUD
/// and debug API keep true values), so gentle gradients read on screen.
const VERTICAL_EXAGGERATION: f32 = 2.0;

// ── Road ──────────────────────────────────────────────────────────────────────
/// Distance between road cross-sections.
const SECTION_STEP_M: f32 = 3.0;
/// Length of road drawn from the window start.
const ROAD_AHEAD_M: f32 = 720.0;
/// Dash + gap period of the centre line; dashes where `d mod 12 < 6`.
const DASH_PERIOD_M: f32 = 12.0;
const DASH_LEN_M: f32 = 6.0;
const LINE_WIDTH_M: f32 = 0.15;
/// Edge-line centres sit this far from the road centre (inside the edge).
const EDGE_LINE_OFFSET_M: f32 = ROAD_HALF_WIDTH_M - LINE_WIDTH_M;
/// Grass shoulder each side of the centre line.
const SHOULDER_HALF_WIDTH_M: f32 = 30.0;
/// Small height steps so road and markings sit above the shoulder.
const ROAD_Y: f32 = 0.01;
const LINE_Y: f32 = 0.02;
const GROUND_Y: f32 = -0.02;

// ── Posts ─────────────────────────────────────────────────────────────────────
const POST_SPACING_M: f32 = 20.0;
const POST_COUNT: usize = 25;
const POST_OFFSET_M: f32 = ROAD_HALF_WIDTH_M + 1.0;
const POST_HEIGHT_M: f32 = 1.2;

// ── Ground ────────────────────────────────────────────────────────────────────
/// Side of the flat grass base plane under everything, following the camera.
const BASE_PLANE_SIZE_M: f32 = 8000.0;
/// The base plane sits this far below the lowest drawn road height.
const BASE_PLANE_DROP_M: f32 = 5.0;

// ── Riders ────────────────────────────────────────────────────────────────────
const RIDER_W_M: f32 = 0.6;
const RIDER_H_M: f32 = 1.8;
/// Staging changes (partner place, camera distance) ease with this time
/// constant: about 95 % in 1 s.
const STAGE_EASE_TAU_S: f32 = 0.33;
/// The partner counts as level with the player within this distance
/// ahead or behind; passing the player he first moves out to the side.
const PASS_CLEAR_M: f32 = 1.0;
/// Sideways distance from the player that counts as out of the way.
const PASS_SIDE_M: f32 = 1.0;
/// The bunch around a caught break: `(metres ahead of the player, lateral
/// offset from the centre line)`, fixed; clear of the player's and the
/// partner's places and of the camera's line of sight.
const BUNCH_OFFSETS_M: [(f32, f32); 12] = [
    (-2.5, -2.2),
    (-3.0, 2.4),
    (0.5, -2.0),
    (1.0, 2.5),
    (3.0, -1.0),
    (4.0, 1.2),
    (5.5, -2.3),
    (6.5, 0.3),
    (8.0, 2.0),
    (9.5, -1.2),
    (11.0, 0.9),
    (13.0, -0.3),
];
/// Mixed flat jersey colours for the bunch (none the player's blue or the
/// partner's red).
const BUNCH_COLOURS: [Color; 12] = [
    Color::srgb(0.95, 0.80, 0.10),
    Color::srgb(0.10, 0.65, 0.30),
    Color::srgb(0.95, 0.95, 0.95),
    Color::srgb(0.10, 0.10, 0.12),
    Color::srgb(0.95, 0.50, 0.10),
    Color::srgb(0.55, 0.25, 0.70),
    Color::srgb(0.10, 0.75, 0.80),
    Color::srgb(0.95, 0.45, 0.70),
    Color::srgb(0.55, 0.40, 0.25),
    Color::srgb(0.60, 0.85, 0.20),
    Color::srgb(0.45, 0.50, 0.55),
    Color::srgb(0.90, 0.75, 0.55),
];

// ── Backdrop ──────────────────────────────────────────────────────────────────
/// Radius of the far-land wall around the camera.
const FAR_LAND_RADIUS_M: f32 = 3000.0;
/// Mountains stand just behind the far land.
const MOUNTAIN_RADIUS_M: f32 = 3001.0;
/// Far land reaches from eye height (the true horizon) this far down.
const FAR_LAND_DEPTH_M: f32 = 600.0;
/// Wall facets of the far-land ring; fine enough that its chords stay
/// behind no mountain (sag 0.11 m).
const FAR_LAND_SEGMENTS: usize = 720;
/// Each ridge edge is split this often, so the mountain ring's chords sag
/// less than its 1 m gap to the far land.
const MOUNTAIN_SUBDIVISION: usize = 8;
/// Mountain peak heights above the horizon (60–200 m); the ring repeats
/// them twice.
const MOUNTAIN_PEAKS_M: [f32; 24] = [
    140.0, 95.0, 180.0, 120.0, 70.0, 160.0, 200.0, 110.0, 85.0, 150.0, 130.0, 190.0, 100.0, 75.0, 170.0, 145.0, 60.0,
    125.0, 185.0, 90.0, 155.0, 115.0, 175.0, 80.0,
];
/// Sideways offsets (m along the ring) of the peaks from an even spacing,
/// cycled, so the ridge does not look machine-made.
const MOUNTAIN_JITTER_M: [f32; 5] = [0.0, 90.0, -70.0, 40.0, -110.0];

const SKY: Color = Color::srgb(0.53, 0.81, 0.92);
const GRASS: Color = Color::srgb(0.30, 0.62, 0.25);
const MOUNTAIN: Color = Color::srgb(0.42, 0.48, 0.58);

/// Drawn height of the road at a distance: the profile's elevation,
/// exaggerated. Every profile height used for drawing goes through here.
fn render_height(profile: &Profile, distance_m: f32) -> f32 {
    VERTICAL_EXAGGERATION * profile.elevation_at(distance_m)
}

/// Logical 2D position → 3D render position at the given height.
pub fn to_render(p: Vec2, height: f32) -> Vec3 {
    Vec3::new(p.x, height, -p.y)
}

#[derive(Component)]
struct ChaseCamera;

/// Far-land ring + mountain ring: follows the camera's position, never its
/// rotation, so the far land's top edge is the true horizon all around.
#[derive(Component)]
struct Backdrop;

/// Flat grass plane under everything; follows the camera's x/z.
#[derive(Component)]
struct BasePlane;

/// Recycled pair of roadside posts.
#[derive(Component)]
struct PostPair {
    slot: usize,
}

/// Upright quad rotated (around Y) to face the camera every frame.
#[derive(Component)]
struct Billboard;

#[derive(Component)]
struct PlayerRider;

#[derive(Component)]
struct PartnerRider;

/// One rider of the bunch: its fixed offset (`BUNCH_OFFSETS_M`).
#[derive(Component)]
struct BunchRider {
    ahead_m: f32,
    lateral_m: f32,
}

/// The eased staging: the partner's place (metres ahead of the player;
/// lateral from the centre line) and the camera's distance behind.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
struct StageEase {
    partner_ahead_m: f32,
    partner_lateral_m: f32,
    camera_behind_m: f32,
}

impl Default for StageEase {
    fn default() -> StageEase {
        let s = Staging::default();
        StageEase {
            partner_ahead_m: s.partner_ahead_m,
            partner_lateral_m: s.partner_lateral_m,
            camera_behind_m: s.camera_behind_m,
        }
    }
}

/// The road strip meshes, rebuilt in place whenever the 3 m window moves.
#[derive(Resource)]
struct RoadStrips {
    shoulder: Handle<Mesh>,
    road: Handle<Mesh>,
    lines: Handle<Mesh>,
    /// Window start the meshes were last built for.
    start_m: Option<f32>,
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(SKY))
            .init_resource::<StageEase>()
            .add_systems(Startup, spawn_scene)
            .add_systems(
                Update,
                (
                    ease_staging,
                    (follow_camera, place_riders, place_bunch, rebuild_road, recycle_posts),
                    (face_camera, follow_backdrop, follow_base_plane),
                )
                    .chain()
                    .in_set(crate::GameSet::Render),
            );
    }
}

// ── Road strips ───────────────────────────────────────────────────────────────

/// One road cross-section: the centre line's position, right vector and
/// drawn height at a distance.
#[derive(Clone, Copy, Debug)]
struct CrossSection {
    distance_m: f32,
    centre: Vec2,
    right: Vec2,
    height: f32,
}

impl CrossSection {
    /// Render position of the point `offset` metres right of the centre line,
    /// `layer` metres above the road.
    fn point(&self, offset: f32, layer: f32) -> Vec3 {
        to_render(self.centre + self.right * offset, self.height + layer)
    }
}

/// Cross-sections every 3 m from `start_d` over `ROAD_AHEAD_M`.
fn cross_sections(profile: &Profile, start_d: f32) -> Vec<CrossSection> {
    let n = (ROAD_AHEAD_M / SECTION_STEP_M).round() as usize;
    (0..=n)
        .map(|i| {
            let d = start_d + i as f32 * SECTION_STEP_M;
            CrossSection {
                distance_m: d,
                centre: profile.centre_at(d),
                right: profile.right_at(d),
                height: render_height(profile, d),
            }
        })
        .collect()
}

/// Whether the centre line is dashed at absolute distance `d`.
fn dash_at(d: f32) -> bool {
    d.rem_euclid(DASH_PERIOD_M) < DASH_LEN_M
}

/// Triangle-list geometry built from bands between cross-sections.
#[derive(Default)]
struct StripMesh {
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl StripMesh {
    /// Quad from offset `left` to `right` (`left < right`) between sections
    /// `a` (near) and `b` (far), wound counter-clockwise seen from above.
    fn band(&mut self, a: &CrossSection, b: &CrossSection, left: f32, right: f32, layer: f32) {
        let base = self.positions.len() as u32;
        for p in [
            a.point(left, layer),
            a.point(right, layer),
            b.point(right, layer),
            b.point(left, layer),
        ] {
            self.positions.push(p.to_array());
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn into_mesh(self) -> Mesh {
        let n = self.positions.len();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n])
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// Shoulder, road and line (edge lines + centre dashes) strips.
///
/// All three are cut laterally at the same offsets (every line edge, road
/// edge and shoulder edge), so wherever two layers overlap they share the
/// exact plan triangulation and stay their layer step apart. A quad spanning
/// a bend on a slope is not planar, so differently cut layers would cross.
fn road_strips(sections: &[CrossSection]) -> [StripMesh; 3] {
    let (mut shoulder, mut road, mut lines) = (StripMesh::default(), StripMesh::default(), StripMesh::default());
    let half_line = LINE_WIDTH_M / 2.0;
    let (e0, e1) = (EDGE_LINE_OFFSET_M - half_line, EDGE_LINE_OFFSET_M + half_line);
    let cuts = [
        -SHOULDER_HALF_WIDTH_M,
        -ROAD_HALF_WIDTH_M,
        -e1,
        -e0,
        -half_line,
        half_line,
        e0,
        e1,
        ROAD_HALF_WIDTH_M,
        SHOULDER_HALF_WIDTH_M,
    ];
    for pair in sections.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        // Sections sit on the 3 m grid and dashes on the 6 m grid, so a band
        // is wholly dash or gap; test its middle.
        let dashed = dash_at((a.distance_m + b.distance_m) / 2.0);
        for cut in cuts.windows(2) {
            let (l, r) = (cut[0], cut[1]);
            shoulder.band(a, b, l, r, GROUND_Y);
            if l >= -ROAD_HALF_WIDTH_M && r <= ROAD_HALF_WIDTH_M {
                road.band(a, b, l, r, ROAD_Y);
            }
            let edge_line = (l, r) == (e0, e1) || (l, r) == (-e1, -e0);
            let dash = l == -half_line && dashed;
            if edge_line || dash {
                lines.band(a, b, l, r, LINE_Y);
            }
        }
    }
    [shoulder, road, lines]
}

// ── Backdrop ──────────────────────────────────────────────────────────────────

/// Render-space point on a ring of `radius` at angle `theta` (clockwise from
/// north, seen from above), `y` above the ring's centre.
fn ring_point(radius: f32, theta: f32, y: f32) -> Vec3 {
    to_render(Vec2::new(theta.sin(), theta.cos()) * radius, y)
}

/// Vertical wall around the ring origin through `(angle, top height)`
/// points (closed), from `base_y` up to each point's height.
fn ring_wall(radius: f32, outline: &[Vec2], base_y: f32) -> Mesh {
    let mut positions = Vec::with_capacity(outline.len() * 2 + 2);
    let mut normals = Vec::with_capacity(outline.len() * 2 + 2);
    for p in outline.iter().chain(outline.first()) {
        // Facing the centre.
        let n = -ring_point(1.0, p.x, 0.0);
        for y in [base_y, p.y] {
            positions.push(ring_point(radius, p.x, y).to_array());
            normals.push(n.to_array());
        }
    }
    let mut indices = Vec::new();
    for i in 0..outline.len() as u32 {
        let (b0, t0, b1, t1) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        indices.extend_from_slice(&[b0, b1, t1, b0, t1, t0]);
    }
    let n = positions.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n])
        .with_inserted_indices(Indices::U32(indices))
}

/// Far land: a level wall from eye height (0) down `FAR_LAND_DEPTH_M`.
fn far_land_mesh() -> Mesh {
    let outline: Vec<Vec2> = (0..FAR_LAND_SEGMENTS)
        .map(|i| Vec2::new(std::f32::consts::TAU * i as f32 / FAR_LAND_SEGMENTS as f32, 0.0))
        .collect();
    ring_wall(FAR_LAND_RADIUS_M, &outline, -FAR_LAND_DEPTH_M)
}

/// Closed ridge outline around the ring as (angle, height above horizon):
/// 48 peaks (`MOUNTAIN_PEAKS_M` twice) evenly spaced with jitter, a lower
/// saddle after each one (the last saddle leads back to the first peak).
fn mountain_outline() -> Vec<Vec2> {
    let count = 2 * MOUNTAIN_PEAKS_M.len();
    let spacing = std::f32::consts::TAU / count as f32;
    let peaks: Vec<Vec2> = (0..count)
        .map(|i| {
            let jitter = MOUNTAIN_JITTER_M[i % MOUNTAIN_JITTER_M.len()] / MOUNTAIN_RADIUS_M;
            Vec2::new(
                spacing * (i as f32 + 0.5) + jitter,
                MOUNTAIN_PEAKS_M[i % MOUNTAIN_PEAKS_M.len()],
            )
        })
        .collect();
    let mut outline = Vec::with_capacity(2 * count);
    for (i, &p) in peaks.iter().enumerate() {
        outline.push(p);
        let mut next = peaks[(i + 1) % count];
        if i + 1 == count {
            next.x += std::f32::consts::TAU;
        }
        outline.push(Vec2::new((p.x + next.x) / 2.0, 0.4 * p.y.min(next.y)));
    }
    outline
}

/// Mountain ring standing on the horizon; its base dips slightly below the
/// horizon, behind the far land, so no sky shows at the seam. Each ridge
/// edge is subdivided so the facets stay behind the far-land wall.
fn mountain_mesh() -> Mesh {
    let outline = mountain_outline();
    let mut fine = Vec::with_capacity(outline.len() * MOUNTAIN_SUBDIVISION);
    for (i, &p) in outline.iter().enumerate() {
        let mut next = outline[(i + 1) % outline.len()];
        if i + 1 == outline.len() {
            next.x += std::f32::consts::TAU;
        }
        for k in 0..MOUNTAIN_SUBDIVISION {
            fine.push(p.lerp(next, k as f32 / MOUNTAIN_SUBDIVISION as f32));
        }
    }
    ring_wall(MOUNTAIN_RADIUS_M, &fine, -5.0)
}

fn unlit(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        unlit: true,
        ..default()
    }
}

/// Distance of the first (rearmost) recycled item: one spacing behind the
/// camera, snapped to the spacing grid so items stay put as the world scrolls.
fn window_start(distance_m: f32, spacing: f32) -> f32 {
    ((distance_m - CAM_BEHIND_MAX_M) / spacing).floor() * spacing - spacing
}

/// Lowest drawn road height over one lap.
fn lowest_render_height(profile: &Profile) -> f32 {
    let steps = profile.total_length_m().ceil() as usize;
    (0..=steps)
        .map(|i| render_height(profile, i as f32))
        .fold(f32::INFINITY, f32::min)
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    profile: Res<Profile>,
) {
    commands.spawn((
        ChaseCamera,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: CAM_FAR_M,
            ..default()
        }),
        Transform::from_xyz(0.0, CAM_HEIGHT_M, CAMERA_BEHIND_M)
            .looking_at(to_render(Vec2::new(0.0, CAM_LOOK_AHEAD_M), 0.0), Vec3::Y),
    ));

    let ground_mat = materials.add(unlit(GRASS));
    commands.spawn((
        BasePlane,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(BASE_PLANE_SIZE_M, BASE_PLANE_SIZE_M))),
        MeshMaterial3d(ground_mat.clone()),
        Transform::from_xyz(0.0, lowest_render_height(&profile) - BASE_PLANE_DROP_M, 0.0),
    ));

    // Road strips: built for real on the first frame by `rebuild_road`.
    let [shoulder, road, lines] =
        road_strips(&cross_sections(&profile, window_start(0.0, SECTION_STEP_M))).map(|m| meshes.add(m.into_mesh()));
    let road_mat = materials.add(unlit(Color::srgb(0.38, 0.38, 0.40)));
    let line_mat = materials.add(unlit(Color::WHITE));
    for (mesh, mat) in [(&shoulder, &ground_mat), (&road, &road_mat), (&lines, &line_mat)] {
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::default(),
            // Bounds change on every rebuild.
            NoFrustumCulling,
        ));
    }
    commands.insert_resource(RoadStrips {
        shoulder,
        road,
        lines,
        start_m: None,
    });

    let backdrop_mat = |color: Color| StandardMaterial {
        cull_mode: None,
        ..unlit(color)
    };
    commands.spawn((
        Backdrop,
        Transform::from_xyz(0.0, CAM_HEIGHT_M, CAMERA_BEHIND_M),
        Visibility::default(),
        children![
            (
                Mesh3d(meshes.add(far_land_mesh())),
                MeshMaterial3d(materials.add(backdrop_mat(GRASS))),
                NoFrustumCulling
            ),
            (
                Mesh3d(meshes.add(mountain_mesh())),
                MeshMaterial3d(materials.add(backdrop_mat(MOUNTAIN))),
                NoFrustumCulling
            ),
        ],
    ));

    let post_mesh = meshes.add(Cuboid::new(0.15, POST_HEIGHT_M, 0.15));
    let post_mat = materials.add(unlit(Color::srgb(0.95, 0.95, 0.95)));
    let cap_mesh = meshes.add(Cuboid::new(0.17, 0.25, 0.17));
    let cap_mat = materials.add(unlit(Color::srgb(0.90, 0.15, 0.10)));
    for slot in 0..POST_COUNT {
        let mut pair = commands.spawn((PostPair { slot }, Transform::default(), Visibility::default()));
        pair.with_children(|p| {
            for side in [-1.0, 1.0] {
                p.spawn((
                    Mesh3d(post_mesh.clone()),
                    MeshMaterial3d(post_mat.clone()),
                    Transform::from_xyz(side * POST_OFFSET_M, POST_HEIGHT_M / 2.0, 0.0),
                ));
                p.spawn((
                    Mesh3d(cap_mesh.clone()),
                    MeshMaterial3d(cap_mat.clone()),
                    Transform::from_xyz(side * POST_OFFSET_M, POST_HEIGHT_M - 0.1, 0.0),
                ));
            }
        });
    }

    let rider_mesh = meshes.add(Rectangle::new(RIDER_W_M, RIDER_H_M));
    let rider_mat = |color: Color| StandardMaterial {
        base_color: color,
        unlit: true,
        cull_mode: None,
        double_sided: true,
        ..default()
    };
    commands.spawn((
        PlayerRider,
        Billboard,
        Mesh3d(rider_mesh.clone()),
        MeshMaterial3d(materials.add(rider_mat(Color::srgb(0.15, 0.35, 0.90)))),
        Transform::from_xyz(0.0, RIDER_H_M / 2.0, 0.0),
    ));
    commands.spawn((
        PartnerRider,
        Billboard,
        Mesh3d(rider_mesh.clone()),
        MeshMaterial3d(materials.add(rider_mat(Color::srgb(0.90, 0.15, 0.15)))),
        Transform::from_xyz(0.0, RIDER_H_M / 2.0, -crate::scenario::PARTNER_AHEAD_M),
    ));
    for ((ahead_m, lateral_m), colour) in BUNCH_OFFSETS_M.into_iter().zip(BUNCH_COLOURS) {
        commands.spawn((
            BunchRider { ahead_m, lateral_m },
            Billboard,
            Mesh3d(rider_mesh.clone()),
            MeshMaterial3d(materials.add(rider_mat(colour))),
            Transform::from_xyz(lateral_m, RIDER_H_M / 2.0, -ahead_m),
            Visibility::Hidden,
        ));
    }
}

/// One frame of easing toward `target`, with `k` the share of the way to go
/// (`1 − e^(−dt/τ)`).
fn ease(current: f32, target: f32, k: f32) -> f32 {
    current + (target - current) * k
}

/// The partner's eased place `(ahead, lateral)`, both relative to the
/// player, one frame of `dt` toward `target`. When he has to pass the
/// player (ahead and target on different sides, or level with the player) he
/// first moves out to the side (at least `PULL_PARTNER_LATERAL_M`), only
/// then forward or back, and back in once clear: a swap, never through the
/// player.
fn ease_partner(current: (f32, f32), target: (f32, f32), dt: f32) -> (f32, f32) {
    let k = 1.0 - (-dt.max(0.0) / STAGE_EASE_TAU_S).exp();
    let (ahead, lateral) = current;
    let same_side =
        (ahead > PASS_CLEAR_M && target.0 > PASS_CLEAR_M) || (ahead < -PASS_CLEAR_M && target.0 < -PASS_CLEAR_M);
    if same_side {
        return (ease(ahead, target.0, k), ease(lateral, target.1, k));
    }
    // Out on the side he is already on (never across the player), else
    // on the target's side.
    let side = if lateral.abs() > 0.05 {
        lateral.signum()
    } else if target.1 < 0.0 {
        -1.0
    } else {
        1.0
    };
    let out = side * target.1.abs().max(PULL_PARTNER_LATERAL_M);
    let lateral = ease(lateral, out, k);
    let ahead = if lateral.abs() >= PASS_SIDE_M {
        ease(ahead, target.0, k)
    } else {
        ahead
    };
    (ahead, lateral)
}

/// Eases the partner's place and the camera distance toward the staging
/// (game time: a pause holds them, a time scale speeds them up).
fn ease_staging(time: Res<Time>, race: Res<RaceState>, scenario: Res<Scenario>, mut eased: ResMut<StageEase>) {
    let dt = time.delta_secs();
    let s = scenario.staging;
    // Lateral relative to the player for the pass logic; staged lateral is
    // relative to the player on a pull, the centre line otherwise.
    let player = race.lateral_m;
    let target_lateral = if s.partner_lateral_m != 0.0 {
        s.partner_lateral_m
    } else {
        -player
    };
    let (ahead, lateral) = ease_partner(
        (eased.partner_ahead_m, eased.partner_lateral_m - player),
        (s.partner_ahead_m, target_lateral),
        dt,
    );
    eased.partner_ahead_m = ahead;
    eased.partner_lateral_m = lateral + player;
    let k = 1.0 - (-dt.max(0.0) / STAGE_EASE_TAU_S).exp();
    eased.camera_behind_m = ease(eased.camera_behind_m, s.camera_behind_m, k);
}

/// Eye above the centre line behind the rider (never lateral, so drifting
/// shows on screen), looking at the centre line ahead. Its pitch follows
/// only half the road's rise to the look point, so climbs rise and descents
/// fall on screen.
fn follow_camera(
    race: Res<RaceState>,
    profile: Res<Profile>,
    eased: Res<StageEase>,
    mut cam: Query<&mut Transform, With<ChaseCamera>>,
) {
    let Ok(mut tf) = cam.single_mut() else { return };
    let eye_d = race.distance_m - eased.camera_behind_m;
    let look_d = race.distance_m + CAM_LOOK_AHEAD_M;
    let ground = render_height(&profile, eye_d);
    let look_h = ground + 1.0 + CAM_SLOPE_FOLLOW * (render_height(&profile, look_d) - ground);
    tf.translation = to_render(profile.centre_at(eye_d), ground + CAM_HEIGHT_M);
    tf.look_at(to_render(profile.centre_at(look_d), look_h), Vec3::Y);
}

fn place_riders(
    race: Res<RaceState>,
    eased: Res<StageEase>,
    profile: Res<Profile>,
    mut player: Query<&mut Transform, (With<PlayerRider>, Without<PartnerRider>)>,
    mut partner: Query<&mut Transform, (With<PartnerRider>, Without<PlayerRider>)>,
) {
    // Feet on the road: the quad's centre is half its height above the
    // terrain at the rider's own distance.
    let height = |d: f32| render_height(&profile, d) + RIDER_H_M / 2.0;
    if let Ok(mut tf) = player.single_mut() {
        let d = race.distance_m;
        let pos = profile.centre_at(d) + profile.right_at(d) * race.lateral_m;
        tf.translation = to_render(pos, height(d));
    }
    if let Ok(mut tf) = partner.single_mut() {
        let d = race.distance_m + eased.partner_ahead_m;
        let pos = profile.centre_at(d) + profile.right_at(d) * eased.partner_lateral_m;
        tf.translation = to_render(pos, height(d));
    }
}

/// The bunch is shown while the staging says so (caught, until the set
/// ends), each rider at its fixed offset from the player.
fn place_bunch(
    race: Res<RaceState>,
    profile: Res<Profile>,
    scenario: Res<Scenario>,
    mut bunch: Query<(&BunchRider, &mut Transform, &mut Visibility)>,
) {
    let shown = scenario.staging.bunch == BunchView::Around;
    for (rider, mut tf, mut vis) in &mut bunch {
        let want = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
        if shown {
            let d = race.distance_m + rider.ahead_m;
            let pos = profile.centre_at(d) + profile.right_at(d) * rider.lateral_m;
            tf.translation = to_render(pos, render_height(&profile, d) + RIDER_H_M / 2.0);
        }
    }
}

/// Rebuilds the road strips in place when the 3 m window start moves or
/// the profile was swapped.
fn rebuild_road(
    race: Res<RaceState>,
    profile: Res<Profile>,
    mut strips: ResMut<RoadStrips>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let start = window_start(race.distance_m, SECTION_STEP_M);
    if strips.start_m == Some(start) && !profile.is_changed() {
        return;
    }
    strips.start_m = Some(start);
    let built = road_strips(&cross_sections(&profile, start));
    for (handle, strip) in [&strips.shoulder, &strips.road, &strips.lines].into_iter().zip(built) {
        if let Some(mut mesh) = meshes.get_mut(handle) {
            *mesh = strip.into_mesh();
        }
    }
}

/// Each pair stands upright at the terrain height of its own distance,
/// turned to the road's heading so its posts sit at `± right * offset`.
fn recycle_posts(race: Res<RaceState>, profile: Res<Profile>, mut posts: Query<(&PostPair, &mut Transform)>) {
    let start = window_start(race.distance_m, POST_SPACING_M);
    for (pair, mut tf) in &mut posts {
        let d = start + pair.slot as f32 * POST_SPACING_M;
        *tf = post_pair_transform(&profile, d);
    }
}

/// Local +X maps to the road's right at `d`: rotating by -heading about Y
/// takes (1, 0, 0) to (cos h, 0, sin h) = render of (cos h, -sin h).
fn post_pair_transform(profile: &Profile, d: f32) -> Transform {
    Transform::from_translation(to_render(profile.centre_at(d), render_height(profile, d)))
        .with_rotation(Quat::from_rotation_y(-profile.heading_at(d)))
}

/// The backdrop ring follows the camera's position, never its rotation.
fn follow_backdrop(
    cam: Query<&Transform, (With<ChaseCamera>, Without<Backdrop>)>,
    mut backdrop: Query<&mut Transform, With<Backdrop>>,
) {
    let (Ok(cam_tf), Ok(mut tf)) = (cam.single(), backdrop.single_mut()) else {
        return;
    };
    tf.translation = cam_tf.translation;
}

/// The base plane follows the camera's x/z; its height is recomputed only
/// when the profile was swapped.
fn follow_base_plane(
    profile: Res<Profile>,
    cam: Query<&Transform, (With<ChaseCamera>, Without<BasePlane>)>,
    mut base: Query<&mut Transform, With<BasePlane>>,
) {
    let (Ok(cam_tf), Ok(mut tf)) = (cam.single(), base.single_mut()) else {
        return;
    };
    tf.translation.x = cam_tf.translation.x;
    tf.translation.z = cam_tf.translation.z;
    if profile.is_changed() {
        tf.translation.y = lowest_render_height(&profile) - BASE_PLANE_DROP_M;
    }
}

fn face_camera(
    cam: Query<&Transform, (With<ChaseCamera>, Without<Billboard>)>,
    mut boards: Query<&mut Transform, With<Billboard>>,
) {
    let Ok(cam_tf) = cam.single() else { return };
    for mut tf in &mut boards {
        let d = cam_tf.translation - tf.translation;
        tf.rotation = Quat::from_rotation_y(d.x.atan2(d.z));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    /// Window starts covering the lap start, bends, the hairpin and the seam.
    const STARTS: [f32; 6] = [-9.0, 0.0, 471.0, 1500.0, 1650.0, 1983.0];

    #[test]
    fn road_edges_are_six_metres_apart_and_centred() {
        let profile = Profile::demo();
        for start in STARTS {
            for cs in cross_sections(&profile, start) {
                let (l, r) = (
                    cs.point(-ROAD_HALF_WIDTH_M, ROAD_Y),
                    cs.point(ROAD_HALF_WIDTH_M, ROAD_Y),
                );
                assert!(close(l.distance(r), 6.0, 1e-3), "width at {}", cs.distance_m);
                let centre = to_render(
                    profile.centre_at(cs.distance_m),
                    render_height(&profile, cs.distance_m) + ROAD_Y,
                );
                assert!(((l + r) / 2.0 - centre).length() <= 1e-3, "centre at {}", cs.distance_m);
            }
        }
    }

    #[test]
    fn sections_are_three_metres_apart() {
        let profile = Profile::demo();
        for start in STARTS {
            let sections = cross_sections(&profile, start);
            assert_eq!(sections.len(), 241);
            for w in sections.windows(2) {
                let (a, b) = (w[0].point(0.0, 0.0), w[1].point(0.0, 0.0));
                let gap = Vec2::new(b.x - a.x, b.z - a.z).length();
                assert!(close(gap, 3.0, 0.01), "gap {gap} at {}", w[0].distance_m);
            }
        }
    }

    #[test]
    fn dashes_follow_absolute_distance() {
        for (d, want) in [
            (0.0, true),
            (5.9, true),
            (6.0, false),
            (11.9, false),
            (12.0, true),
            (-3.0, false),
            (1203.0, true),
        ] {
            assert_eq!(dash_at(d), want, "d {d}");
        }
        let profile = Profile::demo();
        for start in STARTS {
            let sections = cross_sections(&profile, start);
            let bands = sections.len() - 1;
            let dashes = sections
                .windows(2)
                .filter(|w| w[0].distance_m.rem_euclid(12.0) < 6.0)
                .count();
            let [_, _, lines] = road_strips(&sections);
            // Two edge-line quads per band plus one per dashed band.
            assert_eq!(lines.indices.len() / 6, 2 * bands + dashes);
            assert_eq!(dashes, bands / 2);
        }
    }

    /// Every road triangle has a shoulder triangle with the same plan
    /// corners exactly one layer step below it, and every line triangle a
    /// road triangle; so layers never cross, even in bends on slopes.
    #[test]
    fn layers_share_triangulation() {
        let profile = Profile::demo();
        let tris = |m: &StripMesh| -> Vec<[Vec3; 3]> {
            m.indices
                .chunks(3)
                .map(|t| [0, 1, 2].map(|k| Vec3::from(m.positions[t[k] as usize])))
                .collect()
        };
        for start in STARTS {
            let [shoulder, road, lines] = road_strips(&cross_sections(&profile, start));
            let (shoulder, road, lines) = (tris(&shoulder), tris(&road), tris(&lines));
            for (upper, lower, step) in [(&road, &shoulder, ROAD_Y - GROUND_Y), (&lines, &road, LINE_Y - ROAD_Y)] {
                for t in upper.iter() {
                    let below = t.map(|p| p - Vec3::Y * step);
                    assert!(
                        lower
                            .iter()
                            .any(|l| l.iter().zip(&below).all(|(a, b)| (*a - *b).length() < 1e-4)),
                        "start {start}: no matching triangle below {t:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn strips_face_up() {
        let profile = Profile::demo();
        for start in STARTS {
            for strip in road_strips(&cross_sections(&profile, start)) {
                for tri in strip.indices.chunks(3) {
                    let [a, b, c] = [0, 1, 2].map(|k| Vec3::from(strip.positions[tri[k] as usize]));
                    assert!((b - a).cross(c - a).y > 0.0, "start {start}");
                }
            }
        }
    }

    #[test]
    fn posts_stand_at_the_road_edges() {
        let profile = Profile::demo();
        for d in [100.0, 500.0, 760.0, 1580.0, 1990.0, 2010.0] {
            let tf = post_pair_transform(&profile, d);
            let h = render_height(&profile, d);
            for side in [-1.0, 1.0] {
                let got = tf.transform_point(Vec3::new(side * POST_OFFSET_M, 0.0, 0.0));
                let want = to_render(profile.centre_at(d) + profile.right_at(d) * side * POST_OFFSET_M, h);
                assert!((got - want).length() < 1e-3, "d {d} side {side}: {got} vs {want}");
            }
            assert!(tf.rotation.mul_vec3(Vec3::Y).abs_diff_eq(Vec3::Y, 1e-6));
        }
    }

    /// Eases `(ahead, lateral)` toward `target` for `secs` in 60 Hz
    /// frames; the path.
    fn swap_path(from: (f32, f32), target: (f32, f32), secs: f32) -> Vec<(f32, f32)> {
        let mut p = from;
        (0..(secs * 60.0) as usize)
            .map(|_| {
                p = ease_partner(p, target, 1.0 / 60.0);
                p
            })
            .collect()
    }

    #[test]
    fn partner_swaps_round_the_player_in_about_a_second() {
        let (draft, pull) = ((2.0, 0.0), (-2.0, 1.2));
        // `near_s`: when the place is within 0.5 m (a 4 m swap in about a
        // second; from a 12 m gap longer).
        let cases = [
            (draft, pull, 1.5),
            (pull, draft, 1.5),
            ((12.0, 0.0), pull, 2.0),
            (pull, (-6.0, 1.2), 1.5),
            ((-6.0, 1.2), draft, 1.5),
        ];
        for (from, to, near_s) in cases {
            let path = swap_path(from, to, 3.0);
            // Never through the player: level with them only out to the side.
            for p in &path {
                assert!(
                    p.0.abs() >= RIDER_W_M || p.1.abs() >= RIDER_W_M,
                    "{from:?} → {to:?}: {p:?}"
                );
            }
            // Close to the target soon, there after 3 s.
            let at = |s: f32| path[(s * 60.0) as usize - 1];
            let near = |p: (f32, f32), tol: f32| (p.0 - to.0).abs() < tol && (p.1 - to.1).abs() < tol;
            assert!(near(at(near_s), 0.5), "{from:?} → {to:?}: {:?}", at(near_s));
            assert!(near(at(3.0), 0.02), "{from:?} → {to:?}: {:?}", at(3.0));
        }
        // On one side the place eases directly: a growing gap follows.
        let p = ease_partner((2.0, 0.0), (5.0, 0.0), 0.33);
        assert!(
            (p.0 - (2.0 + 3.0 * (1.0 - (-1.0_f32).exp()))).abs() < 0.01 && p.1 == 0.0,
            "{p:?}"
        );
        // Back onto a centre line left of an off-centre player: round the
        // side he is on, not across the player.
        let path = swap_path(pull, (2.0, -0.3), 3.0);
        assert!(
            path.iter().all(|p| p.0.abs() >= RIDER_W_M || p.1 >= RIDER_W_M),
            "{path:?}"
        );
        // Under on a pull he comes up alongside, out to the side.
        let path = swap_path(pull, (0.0, 1.2), 2.0);
        assert!(path.iter().all(|p| p.1 >= 1.2 - 1e-4));
        assert!(path.last().unwrap().0.abs() < 0.01);
    }

    #[test]
    fn bunch_stays_clear_of_the_riders_and_the_camera() {
        let partner_places = [(2.0, 0.0), (-2.0, 1.2), (0.0, 1.2), (-6.0, 1.2)];
        for (ahead, lateral) in BUNCH_OFFSETS_M {
            // Not on the player's or the partner's place (player near the
            // centre line).
            for (pa, pl) in partner_places.into_iter().chain([(0.0, 0.0)]) {
                assert!(
                    (ahead - pa).abs() >= 1.0 || (lateral - pl).abs() >= 1.0,
                    "({ahead}, {lateral}) vs ({pa}, {pl})"
                );
            }
            // Behind the player only well to the side of the camera's line.
            if ahead < 0.0 {
                assert!(
                    ahead > -CAMERA_BEHIND_M + 2.0 && lateral.abs() >= 2.0,
                    "({ahead}, {lateral})"
                );
            }
            assert!(lateral.abs() <= ROAD_HALF_WIDTH_M);
        }
    }

    #[test]
    fn mountain_ring_repeats_the_peaks_in_order() {
        let outline = mountain_outline();
        assert_eq!(outline.len(), 2 * 2 * MOUNTAIN_PEAKS_M.len());
        assert!(outline.windows(2).all(|w| w[0].x < w[1].x));
        assert!(outline[0].x > 0.0 && outline.last().unwrap().x < std::f32::consts::TAU);
        assert!(outline.iter().all(|p| p.y > 0.0 && p.y <= 200.0));
        let peaks: Vec<f32> = outline.iter().step_by(2).map(|p| p.y).collect();
        assert_eq!(&peaks[..24], &MOUNTAIN_PEAKS_M);
        assert_eq!(&peaks[24..], &MOUNTAIN_PEAKS_M);
        assert!(MOUNTAIN_PEAKS_M.iter().all(|&h| (60.0..=200.0).contains(&h)));
    }
}
