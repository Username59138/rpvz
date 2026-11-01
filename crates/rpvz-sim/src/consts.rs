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
/// How long a landed sun stays before vanishing (approx.).
pub const SUN_LIFETIME: u32 = 1000;
/// Sun pickup radius around its centre (approx.).
pub const SUN_PICK_RADIUS: f32 = 40.0;

// --- Plants (original) -----------------------------------------------------
pub const PLANT_HEALTH: i32 = 300;
pub const PEASHOOTER_COST: i32 = 100;
pub const PEASHOOTER_RECHARGE: u32 = 750;
pub const PEASHOOTER_LAUNCH_RATE: i32 = 150;
/// Random shortening of each reload (approx.).
pub const LAUNCH_JITTER: i32 = 15;
/// Pea spawn point relative to the plant cell's left edge (approx.).
pub const PEA_SPAWN_DX: f32 = 50.0;

// --- Projectiles (original) ------------------------------------------------
pub const PEA_DAMAGE: i32 = 20;
pub const PEA_SPEED: f32 = 3.33;
pub const PEA_WIDTH: f32 = 28.0;

// --- Zombies ---------------------------------------------------------------
/// Basic zombie body health (original).
pub const ZOMBIE_HEALTH: i32 = 270;
/// Speed range of a basic zombie, px per tick (original).
pub const ZOMBIE_SPEED_MIN: f32 = 0.23;
pub const ZOMBIE_SPEED_MAX: f32 = 0.32;
/// Eating: `DAMAGE_PER_EAT` every `TICKS_BETWEEN_EATS` ticks (original).
pub const TICKS_BETWEEN_EATS: u32 = 4;
pub const DAMAGE_PER_EAT: i32 = 4;
/// Hit box width (approx.).
pub const ZOMBIE_WIDTH: f32 = 42.0;
/// Zombies appear here (approx.).
pub const ZOMBIE_SPAWN_X: f32 = 790.0;
pub const ZOMBIE_SPAWN_X_RANGE: i32 = 40;
/// Headless zombies lose this much health per tick until they fall (approx.).
pub const HEADLESS_DECAY: i32 = 1;

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
