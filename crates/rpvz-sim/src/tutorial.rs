//! The first-time 1-1 tutorial and the advice messages (the original's
//! `TutorialState`, `DisplayAdvice` and `MessageWidget` timing).
//!
//! While the tutorial runs, zombie waves wait, and no sun falls until the
//! first plant is in. Planting the second peashooter ends it and calls the
//! first wave 99 ticks later.

use std::collections::HashSet;

/// Ticks a non-staying message is shown for.
pub const ADVICE_TICKS: u32 = 500;
/// Duration the original uses for messages that stay until replaced.
pub const ADVICE_STAY: u32 = 10_000;
/// Hint after the packet has recharged, if the player waits this long.
pub const ENOUGH_SUN_WAIT: i32 = 400;
/// First sky sun after the first plant goes in.
pub const FIRST_SUN_AFTER_PLANT: u32 = 400;
/// Zombies come this many ticks after the tutorial ends.
pub const ONSLAUGHT_COUNTDOWN: u32 = 99;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tutorial {
    #[default]
    Off,
    PickUpPeashooter,
    PlantPeashooter,
    RefreshPeashooter,
    Completed,
}

impl Tutorial {
    /// The tutorial still holds the zombies back.
    pub fn holds_waves(self) -> bool {
        matches!(
            self,
            Self::PickUpPeashooter | Self::PlantPeashooter | Self::RefreshPeashooter
        )
    }
}

/// Messages of 1-1, by their `lawnstrings.txt` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Advice {
    ClickSeedPacket,
    ClickOnGrass,
    PlantedPeashooter,
    EnoughSun,
    ClickPeashooter,
    ZombieOnslaught,
    ClickOnSun,
    ClickedOnSun,
    SeedRefresh,
    CantAffordPlant,
}

impl Advice {
    pub fn key(self) -> &'static str {
        match self {
            Self::ClickSeedPacket => "ADVICE_CLICK_SEED_PACKET",
            Self::ClickOnGrass => "ADVICE_CLICK_ON_GRASS",
            Self::PlantedPeashooter => "ADVICE_PLANTED_PEASHOOTER",
            Self::EnoughSun => "ADVICE_ENOUGH_SUN",
            Self::ClickPeashooter => "ADVICE_CLICK_PEASHOOTER",
            Self::ZombieOnslaught => "ADVICE_ZOMBIE_ONSLAUGHT",
            Self::ClickOnSun => "ADVICE_CLICK_ON_SUN",
            Self::ClickedOnSun => "ADVICE_CLICKED_ON_SUN",
            Self::SeedRefresh => "ADVICE_SEED_REFRESH",
            Self::CantAffordPlant => "ADVICE_CANT_AFFORD_PLANT",
        }
    }

    /// Messages with their own help index are shown once per level.
    fn once(self) -> bool {
        matches!(
            self,
            Self::ClickOnSun | Self::ClickedOnSun | Self::SeedRefresh | Self::CantAffordPlant
        )
    }
}

/// The message on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdviceShown {
    pub advice: Advice,
    /// Ticks left; [`ADVICE_STAY`] never counts down.
    pub left: u32,
}

/// Tutorial state of a level.
#[derive(Debug, Clone, Default)]
pub struct TutorialState {
    pub state: Tutorial,
    /// `mTutorialTimer`: counts down to a reminder; -1 when idle.
    pub timer: i32,
    pub advice: Option<AdviceShown>,
    shown: HashSet<Advice>,
    /// Seed packet the tutorial arrow points at.
    pub arrow: Option<usize>,
}

impl TutorialState {
    pub fn new() -> Self {
        Self {
            timer: -1,
            ..Self::default()
        }
    }

    /// `DisplayAdvice`: once-only messages are skipped if already shown.
    pub fn show(&mut self, advice: Advice, stay: bool) {
        if advice.once() && !self.shown.insert(advice) {
            return;
        }
        self.advice = Some(AdviceShown {
            advice,
            left: if stay { ADVICE_STAY } else { ADVICE_TICKS },
        });
    }

    pub fn was_shown(&self, advice: Advice) -> bool {
        self.shown.contains(&advice)
    }

    pub fn clear(&mut self) {
        self.advice = None;
    }

    /// Message countdown, one tick.
    pub fn tick_advice(&mut self) {
        if let Some(a) = &mut self.advice
            && a.left < ADVICE_STAY
        {
            a.left = a.left.saturating_sub(1);
            if a.left == 0 {
                self.advice = None;
            }
        }
    }
}
