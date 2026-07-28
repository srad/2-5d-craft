use crate::adapters::bevy::{
    PendingWorldResource, WorldStateResource,
    camera::{CameraRig, GameCamera, center_camera},
    rendering::RenderCatalog,
    world::WorldEntity,
};
use crate::{AppState, domain::BlockKind};
use avian2d::{math::*, prelude::*};
use bevy::ecs::query::Has;
use bevy::prelude::*;

const WALK_SPEED: f32 = 5.0;
const GROUND_ACCELERATION: f32 = 45.0;
const AIR_ACCELERATION: f32 = 20.0;
const GROUND_DECELERATION: f32 = 60.0;
const GRAVITY: f32 = 25.0;
const JUMP_VELOCITY: f32 = 9.5;
const TERMINAL_VELOCITY: f32 = 30.0;

#[derive(Component)]
#[require(
    RigidBody::Kinematic,
    CustomPositionIntegration,
    SpeculativeMargin(0.0)
)]
pub struct Player;

#[derive(Component)]
pub struct PlayerVisual;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerPart {
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
    Static,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct RespawnPoint(pub Vec2);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotbar {
    pub selected_slot: u8,
}

impl Hotbar {
    pub fn selected_kind(self) -> BlockKind {
        BlockKind::HOTBAR[usize::from(self.selected_slot.saturating_sub(1)).min(7)]
    }
}

#[derive(Component)]
#[component(storage = "SparseSet")]
struct Grounded;

#[derive(Resource, Default)]
struct MovementInput {
    axis: f32,
    jump_queued: bool,
}

#[derive(Component, Default)]
struct AnimationClock {
    elapsed: f32,
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MovementInput>()
            .add_systems(OnEnter(AppState::Playing), spawn_player)
            .add_systems(OnEnter(AppState::Paused), clear_movement_input)
            .add_systems(OnEnter(AppState::MainMenu), clear_movement_input)
            .add_systems(
                PreUpdate,
                (read_movement_input, read_hotbar_input).run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                (
                    update_grounded,
                    apply_controller,
                    move_and_slide,
                    constrain_player,
                )
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(Update, animate_player.run_if(in_state(AppState::Playing)));
    }
}

fn spawn_player(
    mut commands: Commands,
    pending: Option<Res<PendingWorldResource>>,
    world: Option<Res<WorldStateResource>>,
    catalog: Option<Res<RenderCatalog>>,
    existing: Query<(), With<Player>>,
    mut camera: Single<&mut Transform, (With<GameCamera>, Without<Player>)>,
    mut camera_rig: ResMut<CameraRig>,
) {
    if !existing.is_empty() {
        return;
    }
    let (Some(pending), Some(world), Some(catalog)) = (pending, world, catalog) else {
        return;
    };
    let requested = Vec2::new(pending.snapshot.player.local_x, pending.snapshot.player.y);
    let spawn = if world.grid.player_position_is_safe(requested) {
        requested
    } else {
        world.grid.safe_spawn()
    };
    center_camera(spawn, &mut camera, &mut camera_rig);
    let cube = catalog.player_cube.clone();
    commands
        .spawn((
            Player,
            RespawnPoint(spawn),
            Hotbar {
                selected_slot: pending.snapshot.player.selected_slot,
            },
            Collider::rectangle(0.70, 1.80),
            LinearVelocity::ZERO,
            Transform::from_xyz(spawn.x, spawn.y, 0.0),
            Visibility::default(),
            TransformInterpolation,
            AnimationClock::default(),
            WorldEntity,
        ))
        .with_children(|children| {
            children
                .spawn((
                    PlayerVisual,
                    Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_3)),
                    Visibility::default(),
                ))
                .with_children(|parts| {
                    spawn_part(
                        parts,
                        &cube,
                        &catalog.player_materials[1],
                        Vec3::new(0.0, 0.10, 0.0),
                        Vec3::new(0.62, 0.68, 0.38),
                        PlayerPart::Static,
                    );
                    spawn_part(
                        parts,
                        &cube,
                        &catalog.player_materials[0],
                        Vec3::new(0.0, 0.67, 0.0),
                        Vec3::new(0.56, 0.56, 0.50),
                        PlayerPart::Static,
                    );
                    spawn_part(
                        parts,
                        &cube,
                        &catalog.player_materials[3],
                        Vec3::new(0.0, 0.91, -0.01),
                        Vec3::new(0.60, 0.15, 0.54),
                        PlayerPart::Static,
                    );
                    for (x, part) in [(-0.42, PlayerPart::LeftArm), (0.42, PlayerPart::RightArm)] {
                        spawn_part(
                            parts,
                            &cube,
                            &catalog.player_materials[0],
                            Vec3::new(x, 0.08, 0.0),
                            Vec3::new(0.20, 0.64, 0.24),
                            part,
                        );
                    }
                    for (x, part) in [(-0.18, PlayerPart::LeftLeg), (0.18, PlayerPart::RightLeg)] {
                        spawn_part(
                            parts,
                            &cube,
                            &catalog.player_materials[2],
                            Vec3::new(x, -0.55, 0.0),
                            Vec3::new(0.25, 0.58, 0.28),
                            part,
                        );
                        spawn_part(
                            parts,
                            &cube,
                            &catalog.player_materials[4],
                            Vec3::new(x, -0.84, 0.07),
                            Vec3::new(0.27, 0.13, 0.40),
                            PlayerPart::Static,
                        );
                    }
                    for x in [-0.14, 0.14] {
                        spawn_part(
                            parts,
                            &cube,
                            &catalog.player_materials[4],
                            Vec3::new(x, 0.72, 0.27),
                            Vec3::splat(0.055),
                            PlayerPart::Static,
                        );
                    }
                });
        });
    commands.remove_resource::<PendingWorldResource>();
}

fn spawn_part(
    parts: &mut ChildSpawnerCommands,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    translation: Vec3,
    scale: Vec3,
    part: PlayerPart,
) {
    parts.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(translation).with_scale(scale),
        part,
    ));
}

fn read_movement_input(keyboard: Res<ButtonInput<KeyCode>>, mut movement: ResMut<MovementInput>) {
    let left = keyboard.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]);
    let right = keyboard.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]);
    movement.axis = (right as i8 - left as i8) as f32;
    if keyboard.just_pressed(KeyCode::Space) {
        movement.jump_queued = true;
    }
}

fn read_hotbar_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut player: Single<&mut Hotbar, With<Player>>,
) {
    let keys = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
    ];
    for (index, key) in keys.into_iter().enumerate() {
        if keyboard.just_pressed(key) {
            player.selected_slot = index as u8 + 1;
        }
    }
}

fn clear_movement_input(mut movement: ResMut<MovementInput>) {
    *movement = MovementInput::default();
}

fn update_grounded(
    mut commands: Commands,
    players: Query<(Entity, &Transform), With<Player>>,
    spatial_query: SpatialQuery,
) {
    let cast_shape = Collider::rectangle(0.68, 1.76);
    for (entity, transform) in &players {
        let hit = spatial_query.cast_shape(
            &cast_shape,
            transform.translation.xy().adjust_precision(),
            0.0,
            Dir2::NEG_Y,
            &ShapeCastConfig::from_max_distance(0.08),
            &SpatialQueryFilter::from_excluded_entities([entity]),
        );
        if hit.is_some_and(|hit| hit.normal1.y > 0.7) {
            commands.entity(entity).insert(Grounded);
        } else {
            commands.entity(entity).remove::<Grounded>();
        }
    }
}

fn apply_controller(
    time: Res<Time>,
    mut movement: ResMut<MovementInput>,
    mut players: Query<(&mut LinearVelocity, Has<Grounded>), With<Player>>,
) {
    let delta = time.delta_secs();
    for (mut velocity, grounded) in &mut players {
        let target = movement.axis * WALK_SPEED;
        let acceleration = if movement.axis == 0.0 && grounded {
            GROUND_DECELERATION
        } else if grounded {
            GROUND_ACCELERATION
        } else {
            AIR_ACCELERATION
        };
        velocity.x = move_towards(velocity.x, target, acceleration * delta);
        velocity.y = (velocity.y - GRAVITY * delta).max(-TERMINAL_VELOCITY);
        if grounded && movement.jump_queued {
            velocity.y = JUMP_VELOCITY;
        }
    }
    movement.jump_queued = false;
}

fn move_and_slide(
    mut players: Query<(Entity, &Collider, &mut Transform, &mut LinearVelocity), With<Player>>,
    mover: MoveAndSlide,
    time: Res<Time>,
) {
    for (entity, collider, mut transform, mut velocity) in &mut players {
        let output = mover.move_and_slide(
            collider,
            transform.translation.xy().adjust_precision(),
            0.0,
            velocity.0,
            time.delta(),
            &MoveAndSlideConfig::default(),
            &SpatialQueryFilter::from_excluded_entities([entity]),
            |_| MoveAndSlideHitResponse::Accept,
        );
        transform.translation = output.position.f32().extend(transform.translation.z);
        velocity.0 = output.projected_velocity;
    }
}

fn constrain_player(
    mut players: Query<(&mut Transform, &mut LinearVelocity, &RespawnPoint), With<Player>>,
) {
    for (mut transform, mut velocity, respawn) in &mut players {
        if transform.translation.y < -8.0 {
            transform.translation.x = respawn.0.x;
            transform.translation.y = respawn.0.y;
            velocity.0 = Vector::ZERO;
        }
    }
}

fn animate_player(
    time: Res<Time>,
    mut player: Single<(&LinearVelocity, &mut AnimationClock), With<Player>>,
    mut visual: Single<&mut Transform, (With<PlayerVisual>, Without<PlayerPart>)>,
    mut parts: Query<(&PlayerPart, &mut Transform), Without<PlayerVisual>>,
) {
    player.1.elapsed += time.delta_secs();
    if player.0.x.abs() > 0.05 {
        visual.rotation = Quat::from_rotation_y(player.0.x.signum() * std::f32::consts::FRAC_PI_3);
    }
    let walking = player.0.x.abs() > 0.2;
    let jumping = player.0.y.abs() > 0.2;
    let swing = if walking {
        (player.1.elapsed * 9.0).sin() * 0.65
    } else {
        0.0
    };
    for (part, mut transform) in &mut parts {
        transform.rotation = match part {
            PlayerPart::LeftArm => Quat::from_rotation_x(if jumping { -0.8 } else { swing }),
            PlayerPart::RightArm => Quat::from_rotation_x(if jumping { -0.8 } else { -swing }),
            PlayerPart::LeftLeg => Quat::from_rotation_x(if jumping { 0.35 } else { -swing }),
            PlayerPart::RightLeg => Quat::from_rotation_x(if jumping { -0.35 } else { swing }),
            PlayerPart::Static => Quat::IDENTITY,
        };
        if matches!(part, PlayerPart::LeftArm | PlayerPart::RightArm) {
            transform.translation.y = 0.08;
        }
    }
}

pub fn move_towards(current: f32, target: f32, maximum_delta: f32) -> f32 {
    if (target - current).abs() <= maximum_delta {
        target
    } else {
        current + (target - current).signum() * maximum_delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn acceleration_is_bounded_and_reaches_target() {
        assert_eq!(move_towards(0.0, 5.0, 2.0), 2.0);
        assert_eq!(move_towards(4.0, 5.0, 2.0), 5.0);
        assert_eq!(move_towards(1.0, -5.0, 2.0), -1.0);
    }

    #[test]
    fn every_hotbar_slot_maps_to_its_catalog_item() {
        for slot in 1..=8 {
            let hotbar = Hotbar {
                selected_slot: slot,
            };
            assert_eq!(hotbar.selected_kind().def().hotbar_slot, Some(slot));
        }
    }

    #[test]
    fn tuning_values_have_expected_relationships() {
        const {
            assert!(GROUND_ACCELERATION > AIR_ACCELERATION);
            assert!(GROUND_DECELERATION > GROUND_ACCELERATION);
            assert!(JUMP_VELOCITY > WALK_SPEED);
            assert!(TERMINAL_VELOCITY > JUMP_VELOCITY);
        }
    }

    #[test]
    fn grounded_controller_accelerates_and_consumes_jump() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_secs_f32(1.0 / 64.0),
            ))
            .init_resource::<MovementInput>()
            .add_systems(Update, apply_controller);
        app.update();
        {
            let mut input = app.world_mut().resource_mut::<MovementInput>();
            input.axis = 1.0;
            input.jump_queued = true;
        }
        let player = app
            .world_mut()
            .spawn((Player, Grounded, LinearVelocity::ZERO))
            .id();

        app.update();

        let velocity = app.world().get::<LinearVelocity>(player).unwrap();
        assert!(velocity.x > 0.0);
        assert_eq!(velocity.y, JUMP_VELOCITY);
        assert!(!app.world().resource::<MovementInput>().jump_queued);
    }
}
