//! What the 1-1 tutorial shows: the advice message, the arrow over the seed
//! packet, the flashing sod strip, and the flash of a recharged packet.
//! The tutorial itself (state, timing, which message) is in `rpvz_sim`.

use bevy::prelude::*;
use bevy::sprite::Anchor;
use rpvz_sim::{Event, Tutorial};

use crate::board::{Board, BoardEntity, packet_pos};
use crate::particles::spawn_particles;
use crate::reanim::to_world;
use crate::state::GameState;
use crate::strings::Strings;
use crate::text::{LoadedFont, PopText, TextAlign};
use crate::ui::UiAssets;

pub struct AdvicePlugin;

impl Plugin for AdvicePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (message, arrow, sod_flash, packet_flash)
                .after(crate::board::BoardSet)
                .run_if(in_state(GameState::Adventure))
                .run_if(resource_exists::<Board>),
        );
    }
}

/// Tutorial messages: a dark band across the lawn with centred text.
const BAND: Rect = Rect {
    min: Vec2::new(0.0, 400.0),
    max: Vec2::new(800.0, 510.0),
};
const TEXT_DY: f32 = -4.0;
const FONT: &str = "FONT_HOUSEOFTERROR28";
/// Text colour; its alpha pulses between 192 and 255 every 75 ticks.
const TEXT_RGB: (u8, u8, u8) = (253, 245, 173);
const MIN_ALPHA: f32 = 192.0;
const PULSE_TICKS: u32 = 75;
/// Band of black at alpha 128 in sRGB terms (blending is linear here).
const BAND_COLOR: Color = Color::srgba(0.0, 0.0, 0.0, 0.78);
const Z_MESSAGE: f32 = 96.0;
const Z_ARROW: f32 = 97.0;
const Z_SOD_FLASH: f32 = 1.5;
const Z_PACKET_FLASH: f32 = 85.0;

#[derive(Component)]
struct MessageBand;

#[derive(Component)]
struct MessageText(String);

#[derive(Component)]
struct TutorialArrow(usize);

#[derive(Component)]
struct SodFlash;

/// `GetFlashingColor(counter, 75)`: grey level (0..1) bouncing between
/// 255 and 55.
pub fn flashing_gray(tick: u32) -> f32 {
    let age = (tick % PULSE_TICKS) as i32;
    let half = PULSE_TICKS as i32 / 2;
    (200 * (half - age).abs() / half + 55).clamp(0, 255) as f32 / 255.0
}

/// `TodAnimateCurve(75, 0, tick % 75, 192, 255, CURVE_BOUNCE_SLOW_MIDDLE)`.
fn pulse_alpha(tick: u32) -> f32 {
    let age = (tick % PULSE_TICKS) as f32;
    let t = ((PULSE_TICKS as f32 - age) / PULSE_TICKS as f32).clamp(0.0, 1.0);
    let bounce = 1.0 - (2.0 * t - 1.0).abs();
    let w = 2.0 * bounce - bounce * bounce;
    (MIN_ALPHA + (255.0 - MIN_ALPHA) * w).round()
}

/// First baseline of `lines` lines centred in the band (TodDrawStringWrapped
/// with vertical centring; approx. line metrics).
fn first_baseline(font: &LoadedFont, lines: usize) -> f32 {
    let height = lines as f32 * font.line_height();
    BAND.min.y + TEXT_DY + (BAND.height() - height) / 2.0 + font.ascent()
}

#[allow(clippy::type_complexity)]
fn message(
    mut commands: Commands,
    board: Res<Board>,
    strings: Res<Strings>,
    mut ui: UiAssets,
    mut band: Query<&mut Visibility, (With<MessageBand>, Without<MessageText>)>,
    mut text: Query<(
        &mut MessageText,
        &mut PopText,
        &mut Transform,
        &mut Visibility,
    )>,
) {
    let sim = &board.sim;
    let shown = sim.tutorial.advice.map(|a| strings.get(a.advice.key()));
    if band.is_empty() {
        let w = to_world(BAND.min);
        commands.spawn((
            MessageBand,
            BoardEntity,
            Sprite::from_color(BAND_COLOR, BAND.size()),
            Anchor::TOP_LEFT,
            Transform::from_xyz(w.x, w.y, Z_MESSAGE),
            Visibility::Hidden,
        ));
        commands.spawn((
            MessageText(String::new()),
            BoardEntity,
            PopText::new(FONT, "", Color::WHITE, TextAlign::Center),
            Transform::from_xyz(0.0, 0.0, Z_MESSAGE + 0.5),
            Visibility::Hidden,
        ));
        return;
    }
    let vis = if shown.is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut band {
        *v = vis;
    }
    let Ok((mut current, mut pop, mut transform, mut v)) = text.single_mut() else {
        return;
    };
    *v = vis;
    let Some(label) = shown else { return };
    if current.0 != label {
        let boxes = ui.cfg.boxes;
        let lines = label.lines().count().max(1);
        let y = ui
            .fonts
            .get(FONT, &ui.library, &mut ui.images, !boxes)
            .map_or(BAND.center().y, |f| first_baseline(&f, lines));
        let w = to_world(Vec2::new(BAND.center().x, y));
        transform.translation.x = w.x;
        transform.translation.y = w.y;
        pop.text = label.clone();
        current.0 = label;
    }
    let (r, g, b) = TEXT_RGB;
    let color = Color::srgba_u8(r, g, b, pulse_alpha(sim.tick) as u8);
    if pop.color != color {
        pop.color = color;
    }
}

/// The bouncing arrow (`SeedPacketPick` particles) over the packet to click.
fn arrow(mut commands: Commands, board: Res<Board>, arrows: Query<(Entity, &TutorialArrow)>) {
    let want = board.sim.tutorial.arrow;
    let mut have = None;
    for (e, a) in &arrows {
        if Some(a.0) == want {
            have = Some(a.0);
        } else {
            commands.entity(e).despawn();
        }
    }
    if let (Some(packet), None) = (want, have)
        && let Some(def) = board.effects.get(crate::board::ARROW_EFFECT)
    {
        let e = spawn_particles(&mut commands, def.clone(), packet_pos(packet), Z_ARROW, 1);
        commands
            .entity(e)
            .insert((TutorialArrow(packet), BoardEntity));
    }
}

/// While the player holds the first peashooter, the sod strip flashes
/// (drawn again, darkened by `GetFlashingColor`).
fn sod_flash(
    mut commands: Commands,
    board: Res<Board>,
    mut ui: UiAssets,
    mut flash: Query<(&mut Sprite, &mut Visibility), With<SodFlash>>,
) {
    let sim = &board.sim;
    let on = sim.tutorial.state == Tutorial::PlantPeashooter;
    if flash.is_empty() {
        let boxes = ui.cfg.boxes;
        let Some(cached) =
            ui.cache
                .get(crate::board::SOD_IMAGE, &ui.library, &mut ui.images, !boxes)
        else {
            return;
        };
        let sprite = match cached.handle {
            Some(image) => Sprite { image, ..default() },
            None => Sprite::from_color(Color::NONE, cached.size.as_vec2()),
        };
        let w = to_world(crate::board::SOD_1ROW);
        commands.spawn((
            SodFlash,
            BoardEntity,
            sprite,
            Anchor::TOP_LEFT,
            Transform::from_xyz(w.x, w.y, Z_SOD_FLASH),
            Visibility::Hidden,
        ));
        return;
    }
    for (mut sprite, mut vis) in &mut flash {
        *vis = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if on {
            let g = flashing_gray(sim.tick);
            sprite.color = Color::srgb(g, g, g);
        }
    }
}

/// A packet that finishes recharging flashes (`SeedPacketFlash`).
fn packet_flash(mut commands: Commands, board: Res<Board>) {
    for e in &board.events {
        if let Event::SeedRefreshed(i) = e
            && let Some(def) = board.effects.get(crate::board::FLASH_EFFECT)
        {
            let fx = spawn_particles(
                &mut commands,
                def.clone(),
                packet_pos(*i),
                Z_PACKET_FLASH,
                2,
            );
            commands.entity(fx).insert(BoardEntity);
        }
    }
}
