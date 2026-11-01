//! Deterministic game logic. One [`Sim::step`] is one tick (1/100 s). The
//! same seed and the same commands always give the same game, so balance can
//! be tested without a window.
//!
//! Positions are in board pixels (800×600, y down). A zombie's `x` is the left
//! edge of its hit box.

pub mod consts;

use consts::*;

/// Small deterministic RNG (xorshift64*).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    /// `0..n`.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next_u32() % n }
    }

    /// Inclusive integer range.
    pub fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u32) as i32
    }

    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * (self.next_u32() as f32 / u32::MAX as f32)
    }
}

pub type Id = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlantKind {
    Peashooter,
}

impl PlantKind {
    pub fn cost(self) -> i32 {
        match self {
            PlantKind::Peashooter => PEASHOOTER_COST,
        }
    }

    pub fn recharge(self) -> u32 {
        match self {
            PlantKind::Peashooter => PEASHOOTER_RECHARGE,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plant {
    pub id: Id,
    pub kind: PlantKind,
    pub col: usize,
    pub row: usize,
    pub health: i32,
    /// Ticks until the next shot is allowed.
    pub launch_counter: i32,
}

impl Plant {
    pub fn left(&self) -> f32 {
        LAWN_X + self.col as f32 * CELL_W
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZombieState {
    Walking,
    Eating {
        plant: Id,
    },
    /// Fell over; removed after the death animation.
    Dead {
        ticks: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Zombie {
    pub id: Id,
    pub row: usize,
    pub x: f32,
    pub health: i32,
    pub speed: f32,
    pub state: ZombieState,
    pub has_arm: bool,
    pub has_head: bool,
    eat_timer: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pea {
    pub id: Id,
    pub row: usize,
    pub x: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SunState {
    Falling,
    Resting { age: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sun {
    pub id: Id,
    /// Centre.
    pub x: f32,
    pub y: f32,
    pub ground_y: f32,
    pub state: SunState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MowerState {
    Ready,
    Running,
    Gone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mower {
    pub row: usize,
    pub x: f32,
    pub state: MowerState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Won,
    Lost,
}

/// Things that happened during a tick, for sound, effects and animation.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    SunSpawned(Id),
    SunCollected(Id),
    SunExpired(Id),
    Planted(Id),
    PlantEaten(Id),
    Shot { plant: Id, pea: Id },
    PeaHit { pea: Id, zombie: Id },
    ZombieSpawned(Id),
    ArmLost(Id),
    HeadLost(Id),
    ZombieDied(Id),
    ZombieRemoved(Id),
    MowerStarted(usize),
    WaveStarted { wave: u32, last: bool },
    GameOver(Outcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlantError {
    NotEnoughSun,
    Recharging,
    Occupied,
    OutOfBounds,
    /// Row without sod (only one row is playable in 1-1).
    NoSod,
    GameOver,
}

#[derive(Debug, Clone)]
pub struct Level {
    pub start_sun: i32,
    pub rows: [bool; ROWS],
    pub waves: u32,
    pub seeds: Vec<PlantKind>,
}

impl Level {
    /// Adventure 1-1: one sodded middle row, peashooters only.
    pub fn one_one() -> Self {
        Self {
            start_sun: START_SUN_1_1,
            rows: [false, false, true, false, false],
            waves: WAVES_1_1,
            seeds: vec![PlantKind::Peashooter],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Seed {
    pub kind: PlantKind,
    /// Tick from which the seed can be planted again.
    pub ready_at: u32,
    pub last_planted: Option<u32>,
}

impl Seed {
    /// 0.0 = just used, 1.0 = ready.
    pub fn recharge_progress(&self, tick: u32) -> f32 {
        match self.last_planted {
            None => 1.0,
            Some(at) => ((tick - at) as f32 / self.kind.recharge() as f32).min(1.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Sim {
    pub level: Level,
    pub tick: u32,
    pub rng: Rng,
    pub sun: i32,
    pub seeds: Vec<Seed>,
    pub plants: Vec<Plant>,
    pub zombies: Vec<Zombie>,
    pub peas: Vec<Pea>,
    pub suns: Vec<Sun>,
    pub mowers: Vec<Mower>,
    pub outcome: Option<Outcome>,
    pub events: Vec<Event>,
    /// Waves spawned so far.
    pub wave: u32,
    wave_countdown: u32,
    wave_start_health: i32,
    wave_hurry_health: i32,
    sky_sun_countdown: u32,
    suns_fallen: u32,
    next_id: Id,
}

impl Sim {
    pub fn new(level: Level, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let sky = rng.range_i32(SKY_SUN_FIRST_MIN as i32, SKY_SUN_FIRST_MAX as i32) as u32;
        let mowers = (0..ROWS)
            .filter(|&r| level.rows[r])
            .map(|row| Mower {
                row,
                x: MOWER_X,
                state: MowerState::Ready,
            })
            .collect();
        Self {
            sun: level.start_sun,
            seeds: level
                .seeds
                .iter()
                .map(|&kind| Seed {
                    kind,
                    ready_at: 0,
                    last_planted: None,
                })
                .collect(),
            level,
            tick: 0,
            rng,
            plants: Vec::new(),
            zombies: Vec::new(),
            peas: Vec::new(),
            suns: Vec::new(),
            mowers,
            outcome: None,
            events: Vec::new(),
            wave: 0,
            wave_countdown: ZOMBIE_COUNTDOWN_FIRST_WAVE,
            wave_start_health: 0,
            wave_hurry_health: 0,
            sky_sun_countdown: sky,
            suns_fallen: 0,
            next_id: 1,
        }
    }

    fn id(&mut self) -> Id {
        self.next_id += 1;
        self.next_id
    }

    /// Board cell under a board point.
    pub fn cell_at(x: f32, y: f32) -> Option<(usize, usize)> {
        if x < LAWN_X || y < LAWN_Y {
            return None;
        }
        let col = ((x - LAWN_X) / CELL_W) as usize;
        let row = ((y - LAWN_Y) / CELL_H) as usize;
        (col < COLUMNS && row < ROWS).then_some((col, row))
    }

    pub fn row_y(row: usize) -> f32 {
        LAWN_Y + row as f32 * CELL_H
    }

    pub fn can_plant(&self, seed: usize, col: usize, row: usize) -> Result<(), PlantError> {
        if self.outcome.is_some() {
            return Err(PlantError::GameOver);
        }
        let s = self.seeds.get(seed).ok_or(PlantError::OutOfBounds)?;
        if col >= COLUMNS || row >= ROWS {
            return Err(PlantError::OutOfBounds);
        }
        if !self.level.rows[row] {
            return Err(PlantError::NoSod);
        }
        if self.tick < s.ready_at {
            return Err(PlantError::Recharging);
        }
        if self.sun < s.kind.cost() {
            return Err(PlantError::NotEnoughSun);
        }
        if self.plants.iter().any(|p| p.col == col && p.row == row) {
            return Err(PlantError::Occupied);
        }
        Ok(())
    }

    pub fn plant(&mut self, seed: usize, col: usize, row: usize) -> Result<Id, PlantError> {
        self.can_plant(seed, col, row)?;
        let kind = self.seeds[seed].kind;
        self.sun -= kind.cost();
        self.seeds[seed].ready_at = self.tick + kind.recharge();
        self.seeds[seed].last_planted = Some(self.tick);
        let id = self.id();
        self.plants.push(Plant {
            id,
            kind,
            col,
            row,
            health: PLANT_HEALTH,
            launch_counter: PEASHOOTER_LAUNCH_RATE,
        });
        self.events.push(Event::Planted(id));
        Ok(id)
    }

    /// Collects the sun under a board point, if any. Returns its id.
    pub fn collect_sun_at(&mut self, x: f32, y: f32) -> Option<Id> {
        let i = self
            .suns
            .iter()
            .position(|s| (s.x - x).hypot(s.y - y) <= SUN_PICK_RADIUS)?;
        let s = self.suns.remove(i);
        self.sun += SUN_VALUE;
        self.events.push(Event::SunCollected(s.id));
        Some(s.id)
    }

    /// Advances one tick. Events of the tick are in [`Self::events`].
    pub fn step(&mut self) {
        self.events.clear();
        if self.outcome.is_some() {
            return;
        }
        self.tick += 1;
        self.update_sky_sun();
        self.update_suns();
        self.update_waves();
        self.update_plants();
        self.update_peas();
        self.update_zombies();
        self.update_mowers();
        self.check_end();
    }

    fn update_sky_sun(&mut self) {
        self.sky_sun_countdown = self.sky_sun_countdown.saturating_sub(1);
        if self.sky_sun_countdown > 0 {
            return;
        }
        let delay =
            (SKY_SUN_COUNTDOWN + self.suns_fallen * SKY_SUN_SLOWDOWN).min(SKY_SUN_COUNTDOWN_MAX);
        self.sky_sun_countdown = delay + self.rng.below(SKY_SUN_COUNTDOWN_RANGE);
        self.suns_fallen += 1;
        let id = self.id();
        let x = self.rng.range_i32(SKY_SUN_X_MIN, SKY_SUN_X_MAX) as f32;
        let ground_y =
            (SKY_SUN_GROUND_MIN + self.rng.below(SKY_SUN_GROUND_RANGE as u32) as i32) as f32;
        self.suns.push(Sun {
            id,
            x,
            y: SKY_SUN_START_Y,
            ground_y,
            state: SunState::Falling,
        });
        self.events.push(Event::SunSpawned(id));
    }

    fn update_suns(&mut self) {
        let mut expired = Vec::new();
        for s in &mut self.suns {
            match &mut s.state {
                SunState::Falling => {
                    s.y += SKY_SUN_FALL_SPEED;
                    if s.y >= s.ground_y {
                        s.y = s.ground_y;
                        s.state = SunState::Resting { age: 0 };
                    }
                }
                SunState::Resting { age } => {
                    *age += 1;
                    if *age >= SUN_LIFETIME {
                        expired.push(s.id);
                    }
                }
            }
        }
        for id in expired {
            self.suns.retain(|s| s.id != id);
            self.events.push(Event::SunExpired(id));
        }
    }

    fn living_wave_health(&self) -> i32 {
        self.zombies
            .iter()
            .filter(|z| !matches!(z.state, ZombieState::Dead { .. }))
            .map(|z| z.health.max(0))
            .sum()
    }

    fn update_waves(&mut self) {
        if self.wave >= self.level.waves {
            return;
        }
        // A wave worn down enough calls the next one early.
        if self.wave > 0
            && self.living_wave_health() <= self.wave_hurry_health
            && self.wave_countdown > ZOMBIE_COUNTDOWN_HURRY
        {
            self.wave_countdown = ZOMBIE_COUNTDOWN_HURRY;
        }
        self.wave_countdown = self.wave_countdown.saturating_sub(1);
        if self.wave_countdown > 0 {
            return;
        }
        let wave = self.wave;
        self.wave += 1;
        let last = self.wave == self.level.waves;
        // Zombie points grow slowly; a basic zombie costs one point.
        let count = wave / 3 + 1;
        let rows: Vec<usize> = (0..ROWS).filter(|&r| self.level.rows[r]).collect();
        for _ in 0..count {
            let row = rows[self.rng.below(rows.len() as u32) as usize];
            self.spawn_zombie(row);
        }
        self.wave_start_health = self.living_wave_health();
        let share = self.rng.range_f32(WAVE_HEALTH_LOW, WAVE_HEALTH_HIGH);
        self.wave_hurry_health = (self.wave_start_health as f32 * share) as i32;
        self.wave_countdown = ZOMBIE_COUNTDOWN + self.rng.below(ZOMBIE_COUNTDOWN_RANGE);
        self.events.push(Event::WaveStarted {
            wave: self.wave,
            last,
        });
    }

    pub fn spawn_zombie(&mut self, row: usize) -> Id {
        let id = self.id();
        let x = ZOMBIE_SPAWN_X + self.rng.below(ZOMBIE_SPAWN_X_RANGE as u32) as f32;
        let speed = self.rng.range_f32(ZOMBIE_SPEED_MIN, ZOMBIE_SPEED_MAX);
        self.zombies.push(Zombie {
            id,
            row,
            x,
            health: ZOMBIE_HEALTH,
            speed,
            state: ZombieState::Walking,
            has_arm: true,
            has_head: true,
            eat_timer: 0,
        });
        self.events.push(Event::ZombieSpawned(id));
        id
    }

    fn update_plants(&mut self) {
        let mut shots = Vec::new();
        for p in &mut self.plants {
            p.launch_counter -= 1;
            if p.launch_counter > 0 {
                continue;
            }
            let x = p.left();
            let target = self.zombies.iter().any(|z| {
                z.row == p.row
                    && !matches!(z.state, ZombieState::Dead { .. })
                    && z.x + ZOMBIE_WIDTH > x
                    && z.x < BOARD_W
            });
            if target {
                shots.push((p.id, p.row, x + PEA_SPAWN_DX));
                p.launch_counter = PEASHOOTER_LAUNCH_RATE;
            } else {
                // Keep checking every tick until something shows up.
                p.launch_counter = 0;
            }
        }
        for (plant, row, x) in shots {
            let jitter = self.rng.below(LAUNCH_JITTER as u32 + 1) as i32;
            if let Some(p) = self.plants.iter_mut().find(|p| p.id == plant) {
                p.launch_counter -= jitter;
            }
            let id = self.id();
            self.peas.push(Pea { id, row, x });
            self.events.push(Event::Shot { plant, pea: id });
        }
    }

    fn update_peas(&mut self) {
        let mut hits = Vec::new();
        for pea in &mut self.peas {
            pea.x += PEA_SPEED;
            let target = self
                .zombies
                .iter()
                .filter(|z| {
                    z.row == pea.row
                        && !matches!(z.state, ZombieState::Dead { .. })
                        && pea.x + PEA_WIDTH >= z.x
                        && pea.x <= z.x + ZOMBIE_WIDTH
                })
                .min_by(|a, b| a.x.total_cmp(&b.x));
            if let Some(z) = target {
                hits.push((pea.id, z.id));
            }
        }
        for (pea, zombie) in hits {
            self.peas.retain(|p| p.id != pea);
            self.events.push(Event::PeaHit { pea, zombie });
            self.damage_zombie(zombie, PEA_DAMAGE);
        }
        self.peas.retain(|p| p.x < BOARD_W);
    }

    pub fn damage_zombie(&mut self, id: Id, amount: i32) {
        let Some(z) = self.zombies.iter_mut().find(|z| z.id == id) else {
            return;
        };
        if matches!(z.state, ZombieState::Dead { .. }) {
            return;
        }
        z.health -= amount;
        let mut events = Vec::new();
        if z.has_arm && z.health < ZOMBIE_HEALTH * 2 / 3 {
            z.has_arm = false;
            events.push(Event::ArmLost(id));
        }
        if z.has_head && z.health < ZOMBIE_HEALTH / 3 {
            z.has_head = false;
            events.push(Event::HeadLost(id));
        }
        if z.health <= 0 {
            z.state = ZombieState::Dead { ticks: 0 };
            events.push(Event::ZombieDied(id));
        }
        self.events.extend(events);
    }

    fn update_zombies(&mut self) {
        let mut eaten = Vec::new();
        let mut decayed = Vec::new();
        for z in &mut self.zombies {
            match &mut z.state {
                ZombieState::Dead { ticks } => {
                    *ticks += 1;
                    continue;
                }
                ZombieState::Eating { plant } => {
                    let plant_id = *plant;
                    match self.plants.iter_mut().find(|p| p.id == plant_id) {
                        Some(p) => {
                            z.eat_timer += 1;
                            if z.eat_timer >= TICKS_BETWEEN_EATS {
                                z.eat_timer = 0;
                                p.health -= DAMAGE_PER_EAT;
                                if p.health <= 0 {
                                    eaten.push(p.id);
                                    z.state = ZombieState::Walking;
                                }
                            }
                        }
                        None => z.state = ZombieState::Walking,
                    }
                }
                ZombieState::Walking => {
                    z.x -= z.speed;
                    // Bite the first plant the zombie's front reaches.
                    let front = z.x;
                    if let Some(p) = self
                        .plants
                        .iter()
                        .filter(|p| {
                            p.row == z.row
                                && front <= p.left() + CELL_W - 20.0
                                && front >= p.left() - 10.0
                        })
                        .max_by(|a, b| a.left().total_cmp(&b.left()))
                    {
                        z.state = ZombieState::Eating { plant: p.id };
                        z.eat_timer = 0;
                    }
                }
            }
            if !z.has_head {
                decayed.push(z.id);
            }
        }
        for id in eaten {
            self.plants.retain(|p| p.id != id);
            self.events.push(Event::PlantEaten(id));
        }
        for id in decayed {
            self.damage_zombie(id, HEADLESS_DECAY);
        }
        // Death animations last about two seconds.
        let removed: Vec<Id> = self
            .zombies
            .iter()
            .filter(|z| matches!(z.state, ZombieState::Dead { ticks } if ticks >= 200))
            .map(|z| z.id)
            .collect();
        for id in removed {
            self.zombies.retain(|z| z.id != id);
            self.events.push(Event::ZombieRemoved(id));
        }
    }

    fn update_mowers(&mut self) {
        let mut kills = Vec::new();
        for m in &mut self.mowers {
            match m.state {
                MowerState::Ready => {
                    let triggered = self.zombies.iter().any(|z| {
                        z.row == m.row
                            && !matches!(z.state, ZombieState::Dead { .. })
                            && z.x < MOWER_TRIGGER_X
                    });
                    if triggered {
                        m.state = MowerState::Running;
                        self.events.push(Event::MowerStarted(m.row));
                    }
                }
                MowerState::Running => {
                    m.x += MOWER_SPEED;
                    for z in &self.zombies {
                        if z.row == m.row
                            && !matches!(z.state, ZombieState::Dead { .. })
                            && z.x <= m.x + 60.0
                            && z.x + ZOMBIE_WIDTH >= m.x
                        {
                            kills.push(z.id);
                        }
                    }
                    if m.x > BOARD_W {
                        m.state = MowerState::Gone;
                    }
                }
                MowerState::Gone => {}
            }
        }
        for id in kills {
            self.damage_zombie(id, i32::MAX / 2);
        }
    }

    fn check_end(&mut self) {
        let breached = self.zombies.iter().any(|z| {
            !matches!(z.state, ZombieState::Dead { .. })
                && z.x < HOUSE_X
                && !self
                    .mowers
                    .iter()
                    .any(|m| m.row == z.row && m.state != MowerState::Gone)
        });
        let outcome = if breached {
            Some(Outcome::Lost)
        } else if self.wave >= self.level.waves && self.zombies.is_empty() {
            Some(Outcome::Won)
        } else {
            None
        };
        if let Some(o) = outcome {
            self.outcome = Some(o);
            self.events.push(Event::GameOver(o));
        }
    }

    /// Runs `ticks` steps, collecting all events.
    pub fn run(&mut self, ticks: u32) -> Vec<Event> {
        let mut all = Vec::new();
        for _ in 0..ticks {
            self.step();
            all.append(&mut self.events);
        }
        all
    }
}

#[cfg(test)]
mod tests;
