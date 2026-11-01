//! Game numbers. Times are in ticks (1/100 s), distances in screen pixels of
//! the 800×600 board. Values marked "original" match the original game (taken
//! from public reverse-engineering notes as reference); "approx." ones are
//! stand-ins until they are checked.

/// Ticks per second.
pub const TICKS_PER_SECOND: u32 = 100;

// --- Lawn (original) -------------------------------------------------------
pub const COLUMNS: usize = 9;
pub const ROWS: usize = 5;
pub const LAWN_X: f32 = 40.0;
pub const LAWN_Y: f32 = 80.0;
pub const CELL_W: f32 = 80.0;
/// Day and night lawns; pool levels use 85.
pub const CELL_H: f32 = 100.0;
/// The background is drawn this far left while playing.
pub const BOARD_OFFSET: f32 = 220.0;
pub const BOARD_W: f32 = 800.0;

// --- Sun (original) --------------------------------------------------------
pub const SUN_VALUE: i32 = 25;
/// Starting sun in level 1-1 (other levels start with 50).
pub const START_SUN_1_1: i32 = 150;
pub const SKY_SUN_FIRST_MIN: u32 = 425;
pub const SKY_SUN_FIRST_MAX: u32 = 700;
pub const SKY_SUN_COUNTDOWN: u32 = 425;
pub const SKY_SUN_COUNTDOWN_RANGE: u32 = 275;
pub const SKY_SUN_COUNTDOWN_MAX: u32 = 950;
/// Extra delay per sun already fallen.
pub const SKY_SUN_SLOWDOWN: u32 = 10;
pub const SKY_SUN_X_MIN: i32 = 100;
pub const SKY_SUN_X_MAX: i32 = 649;
pub const SKY_SUN_START_Y: f32 = 60.0;
pub const SKY_SUN_FALL_SPEED: f32 = 0.67;
pub const SKY_SUN_GROUND_MIN: i32 = 300;
pub const SKY_SUN_GROUND_RANGE: i32 = 250;
/// A sun is a 60×60 box (its position is the top-left corner).
pub const SUN_SIZE: f32 = 60.0;
/// Ticks a landed sun stays, then ticks it fades for.
pub const SUN_LIFETIME: u32 = 750;
pub const SUN_FADE: u32 = 15;
/// Clicks count this far around the sun's box.
pub const SUN_CLICK_MARGIN: f32 = 15.0;
/// A collected sun flies to this point (top-left), covering 1/21 of the
/// remaining distance per axis each tick, and scores closer than 8 px.
pub const SUN_BANK: (f32, f32) = (15.0, 0.0);
pub const SUN_FLY_DIVISOR: f32 = 21.0;
pub const SUN_SCORE_DISTANCE: f32 = 8.0;
/// Ticks the sun counter flashes red after a too-expensive click.
pub const OUT_OF_MONEY_TICKS: u32 = 70;

// --- Plants (original) -----------------------------------------------------
pub const PLANT_HEALTH: i32 = 300;
pub const PEASHOOTER_COST: i32 = 100;
pub const PEASHOOTER_RECHARGE: u32 = 750;
pub const PEASHOOTER_LAUNCH_RATE: i32 = 150;
/// Each reload is `launch rate - Rand(15)` ticks (0..=14 shorter).
pub const LAUNCH_JITTER: u32 = 15;
/// A shooter sees zombies whose hit box overlaps `[x + 60, x + 60 + 800)`.
pub const SHOOTER_RANGE_DX: f32 = 60.0;
pub const SHOOTER_RANGE: f32 = 800.0;
/// Ticks from the start of a shot (head animation) until it ends; the pea
/// leaves when the counter reaches [`SHOOTING_FIRE_AT`].
pub const PEASHOOTER_SHOOTING_TICKS: u32 = 33;
pub const SHOOTING_FIRE_AT: u32 = 1;
/// Peas start at the head: stem position + (24, -33) from the plant's
/// top-left.
pub const PEA_ORIGIN_DX: i32 = 24;
pub const PEA_ORIGIN_DY: i32 = -33;
/// Body idle animation speed, frames per second, random per plant.
pub const PLANT_IDLE_RATE_MIN: f32 = 15.0;
pub const PLANT_IDLE_RATE_MAX: f32 = 20.0;
/// Stem position used when no animation data was given: the average over
/// the Peashooter's idle cycle (approx.; the real one sways about 10 px).
pub const DEFAULT_STEM: (f32, f32) = (40.95, 44.64);
/// Frames in the Peashooter's idle loop, when no animation data was given.
pub const DEFAULT_IDLE_FRAMES: usize = 25;

// --- Projectiles (original) ------------------------------------------------
pub const PEA_DAMAGE: i32 = 20;
pub const PEA_SPEED: f32 = 3.33;
/// A pea's hit box is `[x - 15, x + 40)` (40 px wide, 15 px reach back).
pub const PEA_RECT_DX: i32 = -15;
pub const PEA_RECT_W: i32 = 55;

// --- Zombies ---------------------------------------------------------------
/// Basic zombie body health (original).
pub const ZOMBIE_HEALTH: i32 = 270;
/// Speed range of a basic zombie, px per tick (original).
pub const ZOMBIE_SPEED_MIN: f32 = 0.23;
pub const ZOMBIE_SPEED_MAX: f32 = 0.32;
/// Eating: `DAMAGE_PER_EAT` every `TICKS_BETWEEN_EATS` ticks (original).
pub const TICKS_BETWEEN_EATS: u32 = 4;
pub const DAMAGE_PER_EAT: i32 = 4;
/// Hit box `(36, 0, 42, 115)` relative to the zombie's position; the sim
/// keeps the hit box's left edge as the zombie's `x`.
pub const ZOMBIE_WIDTH: f32 = 42.0;
pub const ZOMBIE_RECT_DX: f32 = 36.0;
/// Zombies appear at position `780 + Rand(40)`.
pub const ZOMBIE_SPAWN_X: f32 = 780.0 + ZOMBIE_RECT_DX;
pub const ZOMBIE_SPAWN_X_RANGE: i32 = 40;
/// Headless zombies lose this much health on one tick in
/// [`HEADLESS_DECAY_CHANCE`] until they fall.
pub const HEADLESS_DECAY: i32 = 1;
pub const HEADLESS_DECAY_CHANCE: u32 = 5;

// --- Lawn mowers -----------------------------------------------------------
pub const MOWER_X: f32 = -20.0;
/// A zombie this far left triggers the row's mower (approx.).
pub const MOWER_TRIGGER_X: f32 = 15.0;
pub const MOWER_SPEED: f32 = 3.33;
/// Past this, a zombie in a row without a mower reaches the house (approx.).
pub const HOUSE_X: f32 = -40.0;

// --- Waves (original) ------------------------------------------------------
pub const WAVES_1_1: u32 = 4;
pub const ZOMBIE_COUNTDOWN_FIRST_WAVE: u32 = 1800;
pub const ZOMBIE_COUNTDOWN: u32 = 2500;
pub const ZOMBIE_COUNTDOWN_RANGE: u32 = 600;
/// When a wave is worn down below this share of its starting health, the
/// next one comes sooner.
pub const WAVE_HEALTH_LOW: f32 = 0.5;
pub const WAVE_HEALTH_HIGH: f32 = 0.65;
pub const ZOMBIE_COUNTDOWN_HURRY: u32 = 200;
