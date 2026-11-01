//! The lawn: draws an [`rpvz_sim::Sim`] and feeds it the player's clicks.
//!
//! The simulation runs at 100 ticks per second on its own; this module only
//! mirrors its state into reanims and sprites. Screen positions follow the
//! original layout (seed bank, packets, sod strip); offsets between game
//! objects and their animations are approximate where marked.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::sprite::Anchor;
use rpvz_sim::consts::{BOARD_OFFSET, CELL_H, CELL_W, LAWN_X, TICKS_PER_SECOND};
use rpvz_sim::{Event, Id, Level, MowerState, Outcome, Sim, ZombieState};

use crate::config::Config;
use crate::input::{Pointer, Typed};
use crate::library::Library;
use crate::reanim::{
    Clock, ReanimAlpha, ReanimDef, ReanimOrigin, ReanimPlayer, ReanimScale, ReanimSet,
    spawn_reanim, to_world,
};
use crate::state::GameState;
use crate::text::{PopText, TextAlign};
use crate::ui::UiAssets;

pub struct BoardPlugin;

impl Plugin for BoardPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Adventure), enter)
            .add_systems(OnExit(GameState::Adventure), exit)
            .add_systems(
                Update,
                (input, step, sync_views, hud, outcome)
                    .chain()
                    .before(ReanimSet::Advance)
                    .run_if(in_state(GameState::Adventure)),
            );
    }
}

// Seed bank layout (original).
const PACKET_X: f32 = 85.0;
const PACKET_STEP: f32 = 59.0;
const PACKET_Y: f32 = 8.0;
const PACKET_W: f32 = 50.0;
const PACKET_H: f32 = 70.0;
const SUN_TEXT: Vec2 = Vec2::new(34.0, 78.0);
/// Unusable packets are drawn at half brightness in sRGB, like a 128/255
/// colour multiply. Blending happens in linear space, where that is
/// 0.5^2.2 ≈ 0.22, hence alpha 0.78.
const SHADE: Color = Color::srgba(0.0, 0.0, 0.0, 0.78);
const SOD_1ROW: Vec2 = Vec2::new(239.0 - BOARD_OFFSET, 265.0);

// Placement of animations relative to game objects (approx.).
/// Zombie reanim origin relative to its hit box left edge and row top.
const ZOMBIE_DRAW: Vec2 = Vec2::new(-36.0, -30.0);
/// Typical basic-zombie speed the walk animation was made for.
const ZOMBIE_ANIM_SPEED: f32 = 0.27;
const PEA_DRAW_Y: f32 = 22.0;
/// The sun reanim is drawn around its centre.
const SUN_DRAW: Vec2 = Vec2::ZERO;
const MOWER_DRAW_Y: f32 = 23.0;
/// Where the cursor holds a picked-up plant, from its top-left (approx.).
const HELD_GRIP: Vec2 = Vec2::new(CELL_W / 2.0, CELL_H / 2.0);
/// Plant icon placement and size on a packet.
const PACKET_ICON: Vec2 = Vec2::new(5.0, 13.0);
const PACKET_ICON_SCALE: f32 = 0.5;

// Reanim files. The PC data's `PeaShooter` reanim is the Repeater (head
// leaves, eyebrows); the plain Peashooter is `PeaShooterSingle`.
pub const PEASHOOTER: &str = "peashootersingle";
const ZOMBIE: &str = "zombie";
const SUN: &str = "sun";
const MOWER: &str = "lawnmower";

// Draw order (world z).
const Z_BACKGROUND: f32 = 0.0;
const Z_SOD: f32 = 1.0;
const Z_MOWER: f32 = 5.0;
const Z_PLANT: f32 = 10.0;
const Z_PREVIEW: f32 = 15.5;
const Z_ZOMBIE: f32 = 20.0;
const Z_PEA: f32 = 40.0;
const Z_SUN: f32 = 60.0;
const Z_BANK: f32 = 80.0;
const Z_TEXT: f32 = 90.0;
const Z_HELD: f32 = 95.0;

/// Zombie tracks a basic zombie never shows (hats, other zombies' props).
const BASIC_ZOMBIE_HIDDEN: &[&str] = &[
    "cone",
    "bucket",
    "screendoor",
    "flag",
    "duckytube",
    "whitewater",
    "mustache",
    "paper",
    "tongue",
];
const ARM_TRACKS: &[&str] = &["outerarm_lower", "outerarm_hand"];
const HEAD_TRACKS: &[&str] = &["anim_head1", "anim_head2", "anim_hair", "anim_tongue"];
/// Shown instead of the whole upper arm once the arm is gone.
const ARM_STUMP: (&str, &str) = (
    "Zombie_outerarm_upper",
    "IMAGE_REANIM_ZOMBIE_OUTERARM_UPPER2",
);
/// Whole plant (body and head) in one range, used for packets and previews.
const PLANT_FULL: &str = "anim_full_idle";

#[derive(Resource)]
pub struct Board {
    pub sim: Sim,
    acc: f32,
    selected: Option<usize>,
    views: HashMap<Id, Entity>,
    mowers: HashMap<usize, Entity>,
    defs: HashMap<&'static str, Arc<ReanimDef>>,
    ended_at: Option<f32>,
}

#[derive(Component)]
struct BoardEntity;

#[derive(Component)]
struct ZombieView {
    shown: Option<&'static str>,
    arm: bool,
    head: bool,
}

#[derive(Component)]
struct SunCounter;

/// Darkening over a seed packet: `recharge` covers the part still
/// recharging (from the top), otherwise the whole packet while unusable.
#[derive(Component)]
struct PacketShade {
    packet: usize,
    recharge: bool,
}

/// The translucent plant over the cell it would go to.
#[derive(Component)]
struct Preview;

/// The picked-up plant following the cursor.
#[derive(Component)]
struct Held;

/// Position and visibility of [`Preview`] or [`Held`], and which one it is.
type CursorPlant = (
    &'static mut ReanimOrigin,
    &'static mut Visibility,
    Has<Held>,
);
type IsCursorPlant = Or<(With<Preview>, With<Held>)>;

#[derive(Component)]
struct Banner;

fn load_def(library: &Library, name: &str) -> Option<Arc<ReanimDef>> {
    match library.load_reanim(name) {
        Ok(r) => Some(Arc::new(ReanimDef::new(r))),
        Err(e) => {
            warn!("{e}");
            None
        }
    }
}

/// A sprite of image `id` with its top-left at `pos` (game coordinates).
fn sprite(
    commands: &mut Commands,
    ui: &mut UiAssets,
    id: &str,
    pos: Vec2,
    z: f32,
    rect: Option<Rect>,
) -> Option<Entity> {
    let boxes = ui.cfg.boxes;
    let cached = ui.cache.get(id, &ui.library, &mut ui.images, !boxes)?;
    let size = rect.map_or(cached.size.as_vec2(), |r| r.size());
    let sprite = match cached.handle {
        Some(image) => Sprite {
            image,
            rect,
            ..default()
        },
        None => Sprite::from_color(Color::srgba(0.4, 0.8, 0.5, 0.3), size),
    };
    let w = to_world(pos);
    Some(
        commands
            .spawn((
                sprite,
                Anchor::TOP_LEFT,
                Transform::from_xyz(w.x, w.y, z),
                BoardEntity,
            ))
            .id(),
    )
}

fn packet_pos(i: usize) -> Vec2 {
    Vec2::new(PACKET_X + i as f32 * PACKET_STEP, PACKET_Y)
}

#[allow(clippy::too_many_arguments)]
fn enter(
    mut commands: Commands,
    cfg: Res<Config>,
    mut ui: UiAssets,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let seed = if cfg.automation.active() {
        1
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64)
    };
    let sim = Sim::new(Level::one_one(), seed);
    info!("level 1-1, seed {seed}");

    let mut defs = HashMap::new();
    for name in [PEASHOOTER, ZOMBIE, SUN, MOWER] {
        if let Some(d) = load_def(&ui.library, name) {
            defs.insert(name, d);
        }
    }

    sprite(
        &mut commands,
        &mut ui,
        "IMAGE_BACKGROUND1UNSODDED",
        Vec2::new(-BOARD_OFFSET, 0.0),
        Z_BACKGROUND,
        None,
    );
    sprite(
        &mut commands,
        &mut ui,
        "IMAGE_SOD1ROW",
        SOD_1ROW,
        Z_SOD,
        None,
    );
    sprite(
        &mut commands,
        &mut ui,
        "IMAGE_SEEDBANK",
        Vec2::ZERO,
        Z_BANK,
        None,
    );

    for (i, seed) in sim.seeds.iter().enumerate() {
        let pos = packet_pos(i);
        sprite(
            &mut commands,
            &mut ui,
            "IMAGE_SEEDS",
            pos,
            Z_BANK + 1.0,
            Some(Rect::new(0.0, 0.0, PACKET_W, PACKET_H)),
        );
        if let Some(def) = defs.get(PEASHOOTER) {
            let mut icon = ReanimPlayer::new(def.clone());
            icon.play(PLANT_FULL, false);
            icon.layers[0].finished = true;
            let e = spawn_reanim(
                &mut commands,
                &mut meshes,
                &mut materials,
                icon,
                pos + PACKET_ICON,
                Z_BANK + 2.0,
                false,
            );
            commands
                .entity(e)
                .insert((ReanimScale(PACKET_ICON_SCALE), BoardEntity));
        }
        let cost = to_world(pos + Vec2::new(32.0, 64.0));
        commands.spawn((
            PopText::new(
                "FONT_PICO129",
                seed.kind.cost().to_string(),
                Color::BLACK,
                TextAlign::Center,
            ),
            Transform::from_xyz(cost.x, cost.y, Z_BANK + 3.0),
            BoardEntity,
        ));
        let shade = to_world(pos);
        for (n, recharge) in [false, true].into_iter().enumerate() {
            commands.spawn((
                PacketShade {
                    packet: i,
                    recharge,
                },
                Sprite::from_color(SHADE, Vec2::new(PACKET_W, PACKET_H)),
                Anchor::TOP_LEFT,
                Transform::from_xyz(shade.x, shade.y, Z_BANK + 4.0 + n as f32 * 0.1),
                Visibility::Hidden,
                BoardEntity,
            ));
        }
    }
    let sun = to_world(SUN_TEXT);
    commands.spawn((
        SunCounter,
        PopText::new(
            "FONT_CONTINUUMBOLD14",
            sim.sun.to_string(),
            Color::BLACK,
            TextAlign::Center,
        ),
        Transform::from_xyz(sun.x, sun.y, Z_TEXT),
        BoardEntity,
    ));
    if let Some(def) = defs.get(PEASHOOTER) {
        let mut ghost = ReanimPlayer::new(def.clone());
        ghost.play(PLANT_FULL, true);
        let e = spawn_reanim(
            &mut commands,
            &mut meshes,
            &mut materials,
            ghost,
            Vec2::ZERO,
            Z_PREVIEW,
            false,
        );
        commands
            .entity(e)
            .insert((Preview, ReanimAlpha(0.5), Visibility::Hidden, BoardEntity));
        let mut held = ReanimPlayer::new(def.clone());
        held.play(PLANT_FULL, true);
        let e = spawn_reanim(
            &mut commands,
            &mut meshes,
            &mut materials,
            held,
            Vec2::ZERO,
            Z_HELD,
            false,
        );
        commands
            .entity(e)
            .insert((Held, Visibility::Hidden, BoardEntity));
    }

    commands.insert_resource(Board {
        sim,
        acc: 0.0,
        selected: None,
        views: HashMap::new(),
        mowers: HashMap::new(),
        defs,
        ended_at: None,
    });
}

fn exit(mut commands: Commands, q: Query<Entity, With<BoardEntity>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<Board>();
}

fn input(
    pointer: Res<Pointer>,
    typed: Res<Typed>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut board: ResMut<Board>,
) {
    if typed.escape || mouse.just_pressed(MouseButton::Right) {
        board.selected = None;
    }
    let (Some(pos), true) = (pointer.pos, pointer.just_pressed) else {
        return;
    };
    // Sun first: it floats above everything.
    if board.sim.collect_sun_at(pos.x, pos.y).is_some() {
        return;
    }
    for i in 0..board.sim.seeds.len() {
        let r = Rect::from_corners(packet_pos(i), packet_pos(i) + Vec2::new(PACKET_W, PACKET_H));
        if r.contains(pos) {
            let usable = {
                let sim = &board.sim;
                let s = &sim.seeds[i];
                sim.tick >= s.ready_at && sim.sun >= s.kind.cost()
            };
            board.selected = if board.selected == Some(i) || !usable {
                None
            } else {
                Some(i)
            };
            return;
        }
    }
    if let (Some(seed), Some((col, row))) = (board.selected, Sim::cell_at(pos.x, pos.y)) {
        match board.sim.plant(seed, col, row) {
            Ok(_) => board.selected = None,
            Err(e) => info!("can't plant at {col},{row}: {e:?}"),
        }
    }
}

fn step(clock: Res<Clock>, mut board: ResMut<Board>, mut log: Local<Vec<Event>>) {
    let tick = 1.0 / TICKS_PER_SECOND as f32;
    board.acc += clock.dt;
    log.clear();
    let mut steps = 0;
    while board.acc >= tick && steps < 20 {
        board.acc -= tick;
        board.sim.step();
        log.extend(board.sim.events.iter().cloned());
        steps += 1;
    }
    for e in log.iter() {
        match e {
            Event::WaveStarted { wave, last } => {
                info!("wave {wave}{}", if *last { " (final)" } else { "" })
            }
            Event::GameOver(o) => info!("game over: {o:?}"),
            Event::Planted(_) | Event::SunCollected(_) | Event::MowerStarted(_) => info!("{e:?}"),
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_views(
    mut commands: Commands,
    cfg: Res<Config>,
    mut board: ResMut<Board>,
    mut ui: UiAssets,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut players: Query<(
        &mut ReanimPlayer,
        &mut ReanimOrigin,
        Option<&mut ZombieView>,
    )>,
    mut transforms: Query<&mut Transform>,
) {
    let board = &mut *board;
    let sim = &board.sim;
    let labels = cfg.labels;

    // Despawn views of things that are gone.
    let alive: std::collections::HashSet<Id> = sim
        .plants
        .iter()
        .map(|p| p.id)
        .chain(sim.zombies.iter().map(|z| z.id))
        .chain(sim.peas.iter().map(|p| p.id))
        .chain(sim.suns.iter().map(|s| s.id))
        .collect();
    board.views.retain(|id, e| {
        let keep = alive.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });

    let mut spawn_reanim_view =
        |commands: &mut Commands,
         def: &Arc<ReanimDef>,
         origin: Vec2,
         z: f32,
         setup: &dyn Fn(&mut ReanimPlayer)| {
            let mut player = ReanimPlayer::new(def.clone());
            setup(&mut player);
            let e = spawn_reanim(
                commands,
                &mut meshes,
                &mut materials,
                player,
                origin,
                z,
                labels,
            );
            commands.entity(e).insert(BoardEntity);
            e
        };

    for p in &sim.plants {
        if board.views.contains_key(&p.id) {
            continue;
        }
        let Some(def) = board.defs.get(PEASHOOTER) else {
            continue;
        };
        let origin = Vec2::new(p.left(), Sim::row_y(p.row));
        let e = spawn_reanim_view(&mut commands, def, origin, Z_PLANT + p.row as f32, &|pl| {
            pl.play(PLANT_FULL, true);
        });
        board.views.insert(p.id, e);
    }
    for z in &sim.zombies {
        let origin = Vec2::new(z.x, Sim::row_y(z.row)) + ZOMBIE_DRAW;
        if let Some(&e) = board.views.get(&z.id) {
            if let Ok((mut player, mut o, view)) = players.get_mut(e) {
                o.0 = origin;
                if let Some(mut view) = view {
                    update_zombie(&mut player, &mut view, z);
                }
            }
            continue;
        }
        let Some(def) = board.defs.get(ZOMBIE) else {
            continue;
        };
        let e = spawn_reanim_view(&mut commands, def, origin, Z_ZOMBIE + z.row as f32, &|pl| {
            pl.hide_matching(BASIC_ZOMBIE_HIDDEN);
        });
        commands.entity(e).insert(ZombieView {
            shown: None,
            arm: true,
            head: true,
        });
        board.views.insert(z.id, e);
    }
    for pea in &sim.peas {
        let pos = Vec2::new(pea.x, Sim::row_y(pea.row) + PEA_DRAW_Y);
        match board.views.get(&pea.id) {
            Some(&e) => {
                if let Ok(mut t) = transforms.get_mut(e) {
                    let w = to_world(pos);
                    t.translation.x = w.x;
                    t.translation.y = w.y;
                }
            }
            None => {
                if let Some(e) = sprite(
                    &mut commands,
                    &mut ui,
                    "IMAGE_PROJECTILEPEA",
                    pos,
                    Z_PEA,
                    None,
                ) {
                    board.views.insert(pea.id, e);
                }
            }
        }
    }
    for s in &sim.suns {
        let origin = Vec2::new(s.x, s.y) + SUN_DRAW;
        match board.views.get(&s.id) {
            Some(&e) => {
                if let Ok((_, mut o, _)) = players.get_mut(e) {
                    o.0 = origin;
                }
            }
            None => {
                let Some(def) = board.defs.get(SUN) else {
                    continue;
                };
                let e = spawn_reanim_view(&mut commands, def, origin, Z_SUN, &|_| {});
                board.views.insert(s.id, e);
            }
        }
    }
    for m in &sim.mowers {
        let origin = Vec2::new(m.x, Sim::row_y(m.row) + MOWER_DRAW_Y);
        match board.mowers.get(&m.row) {
            Some(&e) => {
                if m.state == MowerState::Gone {
                    commands.entity(e).despawn();
                    board.mowers.remove(&m.row);
                } else if let Ok((mut player, mut o, _)) = players.get_mut(e) {
                    o.0 = origin;
                    if m.state == MowerState::Running && player.layers[0].finished {
                        player.play("anim_normal", true);
                    }
                }
            }
            None if m.state != MowerState::Gone => {
                let Some(def) = board.defs.get(MOWER) else {
                    continue;
                };
                let e = spawn_reanim_view(&mut commands, def, origin, Z_MOWER, &|pl| {
                    pl.play("anim_normal", false);
                    pl.layers[0].finished = true;
                });
                board.mowers.insert(m.row, e);
            }
            None => {}
        }
    }
}

/// Picks the zombie's animation and hides lost body parts.
fn update_zombie(player: &mut ReanimPlayer, view: &mut ZombieView, z: &rpvz_sim::Zombie) {
    let want: &'static str = match z.state {
        ZombieState::Walking => "anim_walk",
        ZombieState::Eating { .. } => "anim_eat",
        ZombieState::Dead { .. } => "anim_death",
    };
    if view.shown != Some(want) {
        player.play(want, want != "anim_death");
        view.shown = Some(want);
    }
    player.speed = match z.state {
        ZombieState::Walking => z.speed / ZOMBIE_ANIM_SPEED,
        _ => 1.0,
    };
    if view.arm && !z.has_arm {
        player.hide_matching(ARM_TRACKS);
        if let Some(t) = player.def.track_index(ARM_STUMP.0) {
            player.image_overrides.insert(t, ARM_STUMP.1.to_owned());
        }
        view.arm = false;
    }
    if view.head && !z.has_head {
        player.hide_matching(HEAD_TRACKS);
        view.head = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn hud(
    board: Res<Board>,
    pointer: Res<Pointer>,
    mut counter: Query<&mut PopText, With<SunCounter>>,
    mut shades: Query<(&PacketShade, &mut Sprite, &mut Visibility), Without<ReanimOrigin>>,
    mut cursor_plants: Query<CursorPlant, IsCursorPlant>,
) {
    let sim = &board.sim;
    if let Ok(mut t) = counter.single_mut() {
        let s = sim.sun.to_string();
        if t.text != s {
            t.text = s;
        }
    }
    for (shade, mut sprite, mut vis) in &mut shades {
        let seed = &sim.seeds[shade.packet];
        let progress = seed.recharge_progress(sim.tick);
        let usable =
            progress >= 1.0 && sim.sun >= seed.kind.cost() && board.selected != Some(shade.packet);
        let height = if shade.recharge {
            1.0 - progress
        } else if usable {
            0.0
        } else {
            1.0
        };
        sprite.custom_size = Some(Vec2::new(PACKET_W, PACKET_H * height));
        *vis = if height > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    let cell = board.selected.zip(pointer.pos).and_then(|(seed, p)| {
        Sim::cell_at(p.x, p.y).filter(|&(c, r)| sim.can_plant(seed, c, r).is_ok())
    });
    for (mut origin, mut vis, held) in &mut cursor_plants {
        let at = if held {
            board.selected.and(pointer.pos).map(|p| p - HELD_GRIP)
        } else {
            cell.map(|(c, r)| Vec2::new(LAWN_X + c as f32 * CELL_W, Sim::row_y(r)))
        };
        match at {
            Some(at) => {
                origin.0 = at;
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

fn outcome(
    mut commands: Commands,
    clock: Res<Clock>,
    mut board: ResMut<Board>,
    banners: Query<(), With<Banner>>,
    mut next: ResMut<NextState<GameState>>,
) {
    let Some(o) = board.sim.outcome else { return };
    if banners.is_empty() {
        let (text, color) = match o {
            Outcome::Won => ("Level complete!", Color::srgb(1.0, 0.9, 0.4)),
            Outcome::Lost => ("The zombies got in...", Color::srgb(0.9, 0.3, 0.3)),
        };
        commands.spawn((
            Banner,
            BoardEntity,
            PopText::new("FONT_HOUSEOFTERROR28", text, color, TextAlign::Center),
            Transform::from_xyz(0.0, 0.0, 99.0),
        ));
        board.ended_at = Some(clock.elapsed);
    }
    if board.ended_at.is_some_and(|t| clock.elapsed - t > 4.0) {
        next.set(GameState::Menu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Animations and tracks the board looks up by name.
    fn check(library: &Library) {
        let def = |name: &str| ReanimDef::new(library.load_reanim(name).unwrap());
        let plant = def(PEASHOOTER);
        assert!(plant.range(PLANT_FULL).is_some());
        // The Repeater shares the Peashooter's images but has eyebrows.
        assert!(
            plant.track_index("PeaShooter_eyebrow").is_none(),
            "Repeater reanim"
        );
        let zombie = def(ZOMBIE);
        for anim in ["anim_walk", "anim_eat", "anim_death"] {
            assert!(zombie.range(anim).is_some(), "zombie {anim}");
        }
        assert!(zombie.track_index(ARM_STUMP.0).is_some());
        assert!(library.image_path(ARM_STUMP.1).is_some());
        for parts in [ARM_TRACKS, HEAD_TRACKS] {
            for part in parts {
                let found = zombie
                    .reanim
                    .tracks
                    .iter()
                    .any(|t| t.name.to_ascii_lowercase().contains(part));
                assert!(found, "zombie track {part}");
            }
        }
        assert!(def(MOWER).range("anim_normal").is_some());
        assert!(def(SUN).track_index("Sun1").is_some());
        for id in [
            "IMAGE_BACKGROUND1UNSODDED",
            "IMAGE_SOD1ROW",
            "IMAGE_SEEDBANK",
            "IMAGE_SEEDS",
            "IMAGE_PROJECTILEPEA",
        ] {
            assert!(library.image_path(id).is_some(), "{id}");
        }
        for font in [
            "FONT_PICO129",
            "FONT_CONTINUUMBOLD14",
            "FONT_HOUSEOFTERROR28",
        ] {
            assert!(library.font_path(font).is_some(), "{font}");
        }
    }

    #[test]
    fn placeholder_pack_has_board_assets() {
        let dir = std::env::temp_dir().join(format!("rpvz-board-{}", std::process::id()));
        crate::devpack::write(&dir).unwrap();
        check(&Library::from_dir(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn game_data_has_board_assets() {
        let Some(pc) = std::env::var_os("RPVZ_TEST_PC_ASSETS") else {
            return;
        };
        check(&Library::from_dir(std::path::Path::new(&pc)));
    }
}
