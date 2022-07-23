use std::time::Duration;

use bevy::{
    core::{Time, Timer},
    hierarchy::{BuildChildren, DespawnRecursiveExt},
    math::Vec2,
    prelude::{
        default, App, AssetServer, Assets, Bundle, Commands, Component, Entity, EventReader,
        EventWriter, Handle, Local, Plugin, Query, Res, Transform, With,
    },
    sprite::SpriteBundle,
    transform::TransformBundle,
};
use bevy_rapier2d::prelude::*;
use iyes_loopless::prelude::*;
use leafwing_input_manager::prelude::ActionState;

use crate::{
    animation::{Animation, Facing},
    audio::FighterStateEffectsPlayback,
    collisions::BodyLayers,
    consts::{
        self, ATTACK_HEIGHT, ATTACK_LAYER, ATTACK_WIDTH, ITEM_HEIGHT, ITEM_LAYER, ITEM_WIDTH,
        THROW_ITEM_ROTATION_SPEED,
    },
    input::PlayerAction,
    item::{Item, ThrowItemEvent},
    metadata::FighterMeta,
    movement::{MoveInArc, MoveInDirection, Rotate, Target},
    state::State,
    ArrivedEvent, Enemy, GameState, Player,
};

pub struct AttackPlugin;

impl Plugin for AttackPlugin {
    fn build(&self, app: &mut App) {
        // Can't be currently converted to a ConditionSet, since (it seems that) systems inside
        // don't have temporal methods available (e.g. after()).
        app.add_system(player_shoot.run_in_state(GameState::InGame))
            .add_system(player_throw.run_in_state(GameState::InGame))
            .add_system(player_flop.run_in_state(GameState::InGame))
            .add_system(attack_tick.run_in_state(GameState::InGame))
            .add_system(attack_cleanup.run_in_state(GameState::InGame))
            .add_system(
                enemy_attack
                    .run_in_state(GameState::InGame)
                    .after("move_to_target"),
            );
    }
}

#[cfg_attr(feature = "debug", derive(bevy_inspector_egui::Inspectable))]
#[derive(Component)]
pub struct Attack {
    pub damage: i32,
}

#[derive(Component)]
pub struct AttackTimer(pub Timer);

#[derive(Bundle)]
pub struct ShotWeapon {
    #[bundle]
    sprite_bundle: SpriteBundle,
    rotate: Rotate,
    collider: Collider,
    sensor: Sensor,
    events: ActiveEvents,
    collision_types: ActiveCollisionTypes,
    collision_groups: CollisionGroups,
    facing: Facing,
    move_in_direction: MoveInDirection,
    attack: Attack,
    attack_timer: AttackTimer,
}

impl ShotWeapon {
    pub fn new(
        transform: &Transform,
        facing: &Facing,
        dir: Vec2,
        asset_server: &Res<AssetServer>,
    ) -> Self {
        Self {
            sprite_bundle: SpriteBundle {
                texture: asset_server.load("bottled_seaweed11x31.png"),
                transform: Transform::from_xyz(
                    transform.translation.x,
                    transform.translation.y,
                    ATTACK_LAYER,
                ),
                ..default()
            },
            rotate: Rotate {
                speed: THROW_ITEM_ROTATION_SPEED,
                to_right: !facing.is_left(),
            },
            collider: Collider::cuboid(ATTACK_WIDTH / 2., ATTACK_HEIGHT / 2.),
            sensor: Sensor(true),
            events: ActiveEvents::COLLISION_EVENTS,
            collision_types: ActiveCollisionTypes::default() | ActiveCollisionTypes::STATIC_STATIC,
            collision_groups: CollisionGroups::new(BodyLayers::PLAYER_ATTACK, BodyLayers::ENEMY),
            facing: facing.clone(),
            move_in_direction: MoveInDirection(dir * 300.), //TODO: Put the velocity in a cons,
            attack: Attack { damage: 10 },
            attack_timer: AttackTimer(Timer::new(Duration::from_secs(1), false)),
        }
    }
}

#[derive(Bundle)]
pub struct ThrownWeapon {
    #[bundle]
    sprite_bundle: SpriteBundle,
    rotate: Rotate,
    move_in_arc: MoveInArc,
    item: Item,
    collider: Collider,
    sensor: Sensor,
    events: ActiveEvents,
    collision_types: ActiveCollisionTypes,
    collision_groups: CollisionGroups,
    attack: Attack,
}

impl ThrownWeapon {
    pub fn new(angles: (f32, f32), ev: &ThrowItemEvent, asset_server: &AssetServer) -> Self {
        Self {
            sprite_bundle: SpriteBundle {
                texture: asset_server.load("bottled_seaweed11x31.png"),
                transform: Transform::from_xyz(ev.position.x, ev.position.y, ITEM_LAYER),
                ..default()
            },
            rotate: Rotate {
                speed: consts::THROW_ITEM_ROTATION_SPEED,
                to_right: !ev.facing.is_left(),
            },
            move_in_arc: MoveInArc {
                //TODO: Set in consts
                radius: Vec2::new(
                    50.,
                    consts::PLAYER_HEIGHT + consts::THROW_ITEM_Y_OFFSET + consts::ITEM_HEIGHT,
                ),
                speed: consts::THROW_ITEM_SPEED,
                angle: angles.0,
                end_angle: angles.1,
                inverse_direction: ev.facing.is_left(),
                origin: ev.position,
            },
            item: Item,
            collider: Collider::cuboid(ITEM_WIDTH / 2., ITEM_HEIGHT / 2.),
            sensor: Sensor(true),
            events: ActiveEvents::COLLISION_EVENTS,
            collision_types: ActiveCollisionTypes::default() | ActiveCollisionTypes::STATIC_STATIC,
            collision_groups: CollisionGroups::new(BodyLayers::ITEM, BodyLayers::ENEMY),
            attack: Attack {
                damage: consts::THROW_ITEM_DAMAGE,
            },
        }
    }
}

fn player_shoot(
    query: Query<(&Transform, &Facing, &State, &ActionState<PlayerAction>), With<Player>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    for (transform, facing, state, input) in query.iter() {
        if *state != State::Idle && *state != State::Running {
            break;
        }
        if input.just_pressed(PlayerAction::Shoot) {
            let mut dir = Vec2::X;

            if facing.is_left() {
                dir = -dir;
            }

            let shot_weapon = ShotWeapon::new(transform, facing, dir, &asset_server);

            commands.spawn_bundle(shot_weapon);
        }
    }
}

fn player_throw(
    query: Query<(&Transform, Option<&Facing>, &ActionState<PlayerAction>), With<Player>>,
    mut ev_throw_item: EventWriter<ThrowItemEvent>,
) {
    for (transform, facing_option, input) in query.iter() {
        if input.just_pressed(PlayerAction::Throw) {
            let facing = match facing_option {
                Some(f) => f.clone(),
                None => Facing::Right,
            };

            let mut position = transform.translation.truncate();

            //Offset the position depending on the facing
            if facing.is_left() {
                position.x -= consts::THROW_ITEM_X_OFFSET;
            } else {
                position.x += consts::THROW_ITEM_X_OFFSET;
            }

            position.y -= consts::PLAYER_HEIGHT / 2.; //Set to the player feet

            ev_throw_item.send(ThrowItemEvent {
                position,
                facing: facing.clone(),
            })
        }
    }
}

fn player_flop(
    mut commands: Commands,
    mut query: Query<
        (
            Entity,
            &mut State,
            &mut Transform,
            &Animation,
            &Facing,
            &ActionState<PlayerAction>,
            &Handle<FighterMeta>,
        ),
        With<Player>,
    >,
    fighter_assets: Res<Assets<FighterMeta>>,
    time: Res<Time>,
    mut start_y: Local<Option<f32>>,
) {
    for (entity, mut state, mut transform, animation, facing, input, fighter_meta) in
        query.iter_mut()
    {
        if *state != State::Attacking {
            if *state != State::Idle && *state != State::Running {
                return;
            }
            if input.just_pressed(PlayerAction::FlopAttack) {
                state.set(State::Attacking);

                if let Some(fighter) = fighter_assets.get(fighter_meta) {
                    if let Some(effects) = fighter.audio.effect_handles.get(&state) {
                        let fx_playback = FighterStateEffectsPlayback::new(*state, effects.clone());
                        commands.entity(entity).insert(fx_playback);
                    }
                }
            }
        // } else if animation.is_finished() {
        // state.set(State::Idle);
        } else {
            //TODO: Fix hacky way to get a forward jump
            if animation.current_frame < 3 {
                if facing.is_left() {
                    transform.translation.x -= 200. * time.delta_seconds();
                } else {
                    transform.translation.x += 200. * time.delta_seconds();
                }
            }

            // For currently unclear reasons, the first Animation frame may run for less Bevy frames
            // than expected. When this is the case, the player jumps less then it should, netting,
            // at the end of the animation, a slightly negative Y than the beginning, which causes
            // problems. This is a workaround.
            //
            if start_y.is_none() {
                *start_y = Some(transform.translation.y);
            }

            if animation.current_frame < 1 {
                transform.translation.y += 180. * time.delta_seconds();
            } else if animation.current_frame < 3 {
                transform.translation.y -= 90. * time.delta_seconds();
            } else if animation.is_finished() {
                transform.translation.y = start_y.unwrap();
                *start_y = None;
            }
        }
    }
}

fn enemy_attack(
    mut query: Query<(Entity, &mut State, &Handle<FighterMeta>), (With<Enemy>, With<Target>)>,
    mut event_reader: EventReader<ArrivedEvent>,
    mut commands: Commands,
    fighter_assets: Res<Assets<FighterMeta>>,
) {
    for event in event_reader.iter() {
        if let Ok((entity, mut state, fighter_handle)) = query.get_mut(event.0) {
            if *state != State::Attacking {
                if rand::random() && *state != State::Waiting {
                    state.set(State::Waiting);
                } else {
                    state.set(State::Attacking);

                    let attack_entity = commands
                        .spawn_bundle(TransformBundle::default())
                        .insert(Collider::cuboid(ATTACK_WIDTH * 0.8, ATTACK_HEIGHT * 0.8))
                        .insert(Sensor(true))
                        .insert(ActiveEvents::COLLISION_EVENTS)
                        .insert(
                            ActiveCollisionTypes::default() | ActiveCollisionTypes::STATIC_STATIC,
                        )
                        .insert(CollisionGroups::new(
                            BodyLayers::ENEMY_ATTACK,
                            BodyLayers::PLAYER,
                        ))
                        .insert(Attack { damage: 10 })
                        .insert(AttackTimer(Timer::new(
                            Duration::from_secs_f32(0.48),
                            false,
                        )))
                        .id();
                    commands.entity(event.0).push_children(&[attack_entity]);

                    if let Some(fighter) = fighter_assets.get(fighter_handle) {
                        if let Some(effects) = fighter.audio.effect_handles.get(&state) {
                            let fx_playback =
                                FighterStateEffectsPlayback::new(*state, effects.clone());
                            commands.entity(entity).insert(fx_playback);
                        }
                    }
                }
            }
        }
    }
}

fn attack_cleanup(query: Query<(Entity, &AttackTimer), With<Attack>>, mut commands: Commands) {
    for (entity, timer) in query.iter() {
        if timer.0.finished() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

fn attack_tick(mut query: Query<&mut AttackTimer, With<Attack>>, time: Res<Time>) {
    for mut timer in query.iter_mut() {
        timer.0.tick(time.delta());
    }
}
