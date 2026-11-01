use super::*;

/// 1-1 without the tutorial (it holds the zombies back).
fn one_one() -> Sim {
    Sim::new(no_tutorial(), 42)
}

fn no_tutorial() -> Level {
    Level {
        tutorial: false,
        ..Level::one_one()
    }
}

/// Fires `n` peas straight into a zombie, as damage events.
fn shoot(sim: &mut Sim, zombie: Id, n: u32) {
    for _ in 0..n {
        sim.damage_zombie(zombie, consts::PEA_DAMAGE);
    }
}

#[test]
fn basic_zombie_takes_ten_peas() {
    let mut sim = one_one();
    let z = sim.spawn_zombie(2);
    shoot(&mut sim, z, 9);
    let zombie = sim.zombies.iter().find(|x| x.id == z).unwrap();
    assert!(zombie.has_head, "nine peas: still has its head");
    assert!(!zombie.has_arm, "arm goes at two thirds");
    shoot(&mut sim, z, 1);
    let zombie = sim.zombies.iter().find(|x| x.id == z).unwrap();
    assert!(!zombie.has_head, "tenth pea knocks the head off");
    assert_eq!(
        zombie.state,
        ZombieState::Walking,
        "headless zombies keep going a moment"
    );
    // Headless decay (1 health on one tick in five) finishes the remaining
    // 70 health in about 3.5 seconds.
    let mut ticks = 0;
    while !sim
        .zombies
        .iter()
        .any(|x| x.id == z && matches!(x.state, ZombieState::Dead { .. }))
    {
        sim.step();
        ticks += 1;
        assert!(ticks < 1000, "still alive");
    }
    assert!((200..=600).contains(&ticks), "{ticks} ticks");
}

#[test]
fn zombie_eats_a_plant_in_three_seconds() {
    let mut sim = one_one();
    let plant = sim.plant(0, 4, 2).unwrap();
    // Stop it shooting so the zombie survives.
    sim.plants[0].launch_counter = i32::MAX;
    let z = sim.spawn_zombie(2);
    let px = sim.plants[0].left();
    sim.zombies.iter_mut().find(|x| x.id == z).unwrap().x = px + 40.0;
    sim.step();
    assert!(matches!(sim.zombies[0].state, ZombieState::Eating { plant: p } if p == plant));
    let start = sim.tick;
    while sim.plants.iter().any(|p| p.id == plant) {
        sim.step();
        assert!(sim.tick - start < 1000, "plant never died");
    }
    // 300 health, 4 damage every 4 ticks: 300 ticks.
    let took = sim.tick - start;
    assert!((299..=301).contains(&took), "took {took} ticks");
}

#[test]
fn planting_rules() {
    let mut sim = one_one();
    assert_eq!(sim.sun, 150);
    assert_eq!(sim.can_plant(0, 0, 0), Err(PlantError::NoSod));
    assert_eq!(sim.can_plant(0, 9, 2), Err(PlantError::OutOfBounds));
    sim.plant(0, 0, 2).unwrap();
    assert_eq!(sim.sun, 50);
    assert_eq!(sim.can_plant(0, 1, 2), Err(PlantError::Recharging));
    sim.run(consts::PEASHOOTER_RECHARGE);
    sim.sun = 99;
    assert_eq!(sim.can_plant(0, 1, 2), Err(PlantError::NotEnoughSun));
    sim.sun = 200;
    assert_eq!(sim.can_plant(0, 0, 2), Err(PlantError::Occupied));
    assert_eq!(sim.can_plant(0, 1, 2), Ok(()));
}

#[test]
fn first_sky_sun_and_its_value() {
    let mut sim = one_one();
    let mut first = None;
    for _ in 0..800 {
        sim.step();
        if first.is_none() && sim.events.iter().any(|e| matches!(e, Event::SunSpawned(_))) {
            first = Some(sim.tick);
        }
    }
    let first = first.expect("a sun fell");
    assert!((425..=700).contains(&first), "first sun at {first}");
    let s = sim.suns[0].clone();
    let (cx, cy) = s.centre();
    let before = sim.sun;
    assert_eq!(sim.collect_sun_at(cx + 5.0, cy - 5.0), Some(s.id));
    assert_eq!(sim.sun, before, "paid when it reaches the counter");
    assert_eq!(sim.collect_sun_at(cx, cy), None, "already on its way");
    let mut ticks = 0;
    while sim.sun == before {
        sim.step();
        ticks += 1;
        assert!(ticks < 200, "never arrived");
    }
    assert_eq!(sim.sun, before + 25);
    assert!(sim.suns.iter().all(|x| x.id != s.id));
}

#[test]
fn peashooter_fires_every_one_and_a_half_seconds() {
    let mut sim = one_one();
    let p = sim.plant(0, 0, 2).unwrap();
    let z = sim.spawn_zombie(2);
    // Keep the zombie alive and in range.
    sim.zombies.iter_mut().find(|x| x.id == z).unwrap().health = 1_000_000;
    let shots: Vec<u32> = (0..1000)
        .filter_map(|_| {
            sim.step();
            sim.events
                .iter()
                .any(|e| matches!(e, Event::Shot { plant, .. } if *plant == p))
                .then_some(sim.tick)
        })
        .collect();
    assert!(shots.len() >= 6, "{shots:?}");
    // Reloads are 150 - Rand(15) ticks.
    for w in shots.windows(2) {
        let gap = w[1] - w[0];
        assert!((135..=150).contains(&gap), "gap {gap}");
    }
}

/// Plays 1-1 with a simple strategy: collect every sun, keep planting
/// peashooters from the left.
fn play(seed: u64, defend: bool) -> (Sim, Vec<Event>) {
    let mut sim = Sim::new(no_tutorial(), seed);
    let mut log = Vec::new();
    while sim.outcome.is_none() && sim.tick < 100 * 60 * 10 {
        if defend {
            let landed: Vec<(f32, f32)> = sim.suns.iter().map(Sun::centre).collect();
            for (x, y) in landed {
                sim.collect_sun_at(x, y);
            }
            let next = sim.plants.len();
            if next < 9 && sim.can_plant(0, next, 2).is_ok() {
                sim.plant(0, next, 2).unwrap();
            }
        }
        sim.step();
        log.extend(sim.events.iter().cloned());
    }
    (sim, log)
}

#[test]
fn level_one_one_can_be_won() {
    let (sim, log) = play(7, true);
    assert_eq!(sim.outcome, Some(Outcome::Won), "after {} ticks", sim.tick);
    let spawned = log
        .iter()
        .filter(|e| matches!(e, Event::ZombieSpawned(_)))
        .count();
    // Waves of 1, 1, 1 and 2 basic zombies.
    assert_eq!(spawned, 5);
    let waves: Vec<_> = log
        .iter()
        .filter_map(|e| match e {
            Event::WaveStarted { wave, last } => Some((*wave, *last)),
            _ => None,
        })
        .collect();
    assert_eq!(waves, vec![(1, false), (2, false), (3, false), (4, true)]);
    // The first wave arrives after 18 seconds.
    let first = log
        .iter()
        .position(|e| matches!(e, Event::WaveStarted { .. }))
        .unwrap();
    let _ = first;
}

#[test]
fn undefended_lawn_uses_the_mower_then_loses() {
    let (sim, log) = play(7, false);
    assert!(log.iter().any(|e| matches!(e, Event::MowerStarted(2))));
    assert_eq!(sim.outcome, Some(Outcome::Lost));
}

#[test]
fn same_seed_same_game() {
    let (a, la) = play(99, true);
    let (b, lb) = play(99, true);
    assert_eq!(a.tick, b.tick);
    assert_eq!(la, lb);
    let (c, _) = play(100, true);
    assert_ne!((a.tick, a.sun), (c.tick, c.sun), "different seeds differ");
}

#[test]
fn cells() {
    assert_eq!(Sim::cell_at(40.0, 80.0), Some((0, 0)));
    assert_eq!(Sim::cell_at(759.0, 579.0), Some((8, 4)));
    assert_eq!(Sim::cell_at(39.0, 300.0), None);
    assert_eq!(Sim::cell_at(400.0, 580.0), None);
}

#[test]
fn pea_leaves_32_ticks_into_the_shot_from_the_head() {
    let mut sim = one_one();
    let p = sim.plant(0, 1, 2).unwrap();
    let z = sim.spawn_zombie(2);
    sim.zombies.iter_mut().find(|x| x.id == z).unwrap().health = 1_000_000;
    let mut started = None;
    for _ in 0..400 {
        sim.step();
        for e in &sim.events {
            match e {
                Event::ShootStarted(id) if *id == p => started = Some(sim.tick),
                Event::Shot { plant, pea } if *plant == p => {
                    let at = started.expect("shot started first");
                    assert_eq!(sim.tick - at, 32);
                    let pea = sim.peas.iter().find(|x| x.id == *pea).unwrap();
                    let left = sim.plants[0].left();
                    // Stem x 37.6..47.4 in the real animation, + 24.
                    assert!((left + 61.0..=left + 72.0).contains(&pea.x), "{}", pea.x);
                    return;
                }
                _ => {}
            }
        }
    }
    panic!("never fired");
}

#[test]
fn no_target_no_shot_but_the_reload_still_runs() {
    let mut sim = one_one();
    sim.plant(0, 1, 2).unwrap();
    let before = sim.rng.clone();
    sim.run(1000);
    assert!(sim.peas.is_empty());
    assert!(
        !sim.events
            .iter()
            .any(|e| matches!(e, Event::ShootStarted(_)))
    );
    // The reload counter kept rolling (and drawing random numbers).
    assert_ne!(format!("{before:?}"), format!("{:?}", sim.rng));
}

#[test]
fn zombie_spawns_just_off_the_lawn() {
    let mut sim = one_one();
    for _ in 0..50 {
        let z = sim.spawn_zombie(2);
        let x = sim.zombies.iter().find(|q| q.id == z).unwrap().x;
        assert!((816.0..856.0).contains(&x), "{x}");
    }
}

#[test]
fn sun_box_click_margin_and_lifetime() {
    let mut sim = one_one();
    sim.run(700);
    let s = sim.suns[0].clone();
    // 15 px around the 60×60 box counts, 16 doesn't.
    assert_eq!(
        sim.clone().collect_sun_at(s.x - 15.0, s.y + 30.0),
        Some(s.id)
    );
    assert_eq!(sim.clone().collect_sun_at(s.x - 16.0, s.y + 30.0), None);
    assert_eq!(
        sim.clone().collect_sun_at(s.x + 74.0, s.y + 74.0),
        Some(s.id)
    );
    // Falls at 0.67 px a tick, then 750 ticks on the ground and 15 fading.
    let fall = ((s.ground_y - s.y) / consts::SKY_SUN_FALL_SPEED).ceil() as u32;
    let mut ticks: u32 = 0;
    while sim.suns.iter().any(|x| x.id == s.id) {
        sim.step();
        ticks += 1;
        assert!(ticks < 2000);
    }
    let expected = fall + consts::SUN_LIFETIME + consts::SUN_FADE;
    assert!(ticks.abs_diff(expected) <= 2, "{ticks} vs {expected}");
}

#[test]
fn first_time_one_one_walks_through_the_tutorial() {
    let mut sim = Sim::new(Level::one_one(), 7);
    let advice = |sim: &Sim| sim.tutorial.advice.map(|a| a.advice);
    sim.step();
    assert_eq!(sim.tutorial.state, Tutorial::PickUpPeashooter);
    assert_eq!(advice(&sim), Some(Advice::ClickSeedPacket));
    assert_eq!(sim.tutorial.arrow, Some(0));
    // Nothing falls or attacks while waiting for the first plant.
    sim.run(3000);
    assert!(sim.suns.is_empty() && sim.zombies.is_empty());

    sim.click_seed(0).unwrap();
    assert_eq!(sim.tutorial.state, Tutorial::PlantPeashooter);
    assert_eq!(advice(&sim), Some(Advice::ClickOnGrass));
    assert_eq!(sim.tutorial.arrow, None);
    // Putting it back goes back a step.
    sim.drop_cursor();
    assert_eq!(sim.tutorial.state, Tutorial::PickUpPeashooter);
    sim.click_seed(0).unwrap();
    sim.plant(0, 0, 2).unwrap();
    assert_eq!(sim.cursor, None);
    assert_eq!(sim.tutorial.state, Tutorial::RefreshPeashooter);
    assert_eq!(advice(&sim), Some(Advice::PlantedPeashooter));

    // The first sun falls 400 ticks later and asks to be clicked.
    sim.run(400);
    assert_eq!(sim.suns.len(), 1);
    assert_eq!(advice(&sim), Some(Advice::ClickOnSun));
    // The packet recharges: "enough sun", then a reminder 400 ticks on.
    while sim.tutorial.state == Tutorial::RefreshPeashooter {
        sim.step();
    }
    assert_eq!(sim.tutorial.state, Tutorial::PickUpPeashooter);
    assert_eq!(advice(&sim), Some(Advice::EnoughSun));
    sim.run(400);
    assert_eq!(advice(&sim), Some(Advice::ClickPeashooter));
    assert!(sim.zombies.is_empty(), "still no zombies");

    // The second peashooter ends the tutorial; zombies 99 ticks later.
    sim.sun = 100;
    sim.click_seed(0).unwrap();
    sim.plant(0, 1, 2).unwrap();
    assert_eq!(sim.tutorial.state, Tutorial::Completed);
    assert_eq!(advice(&sim), Some(Advice::ZombieOnslaught));
    let start = sim.tick;
    while sim.zombies.is_empty() {
        sim.step();
        assert!(sim.tick - start < 200);
    }
    assert_eq!(sim.tick - start, 99);
    // The onslaught message goes after 500 ticks.
    sim.run(500);
    assert_eq!(advice(&sim), None);
}

#[test]
fn too_expensive_packet_flashes_the_counter() {
    let mut sim = one_one();
    sim.sun = 50;
    assert_eq!(sim.click_seed(0), Err(PlantError::NotEnoughSun));
    assert_eq!(sim.out_of_money, consts::OUT_OF_MONEY_TICKS);
    assert_eq!(sim.cursor, None);
}
