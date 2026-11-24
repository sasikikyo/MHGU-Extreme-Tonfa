//! Dragon Qi for verified monster/part layouts. GU part IDs can share a pool.
const PARTS: &[u8] = include_bytes!("../qi-parts.bin");
const GAINS: &[u8; 117] = include_bytes!("../qi-gains.bin");
const PART_SIZE: usize = 10;
const RECORD: usize = 2 + 9 * PART_SIZE;
const TIMING: &[u8; 8] = include_bytes!("../qi-timing.bin");
fn ticks(index: usize) -> u16 {
    let offset = 2 * index;
    u16::from_le_bytes([TIMING[offset], TIMING[offset + 1]])
}
pub fn resonance_ticks(parts: u32) -> u16 {
    ticks(parts.clamp(1, 3) as usize)
}
const _: () = assert!(PARTS.len() % RECORD == 0);

pub fn shell_gain(shell: u16) -> u16 {
    let index = usize::from(shell);
    let Some(&base) = GAINS.get(index) else { return 0; };
    // 8a031a dispatches type 111 to discharge before ordinary buildup.
    // 11e5123 promotes a zero ordinary shell contribution to one.
    if shell == 0 || discharge_shell(shell) {
        return 0;
    }
    u16::from(base.max(1))
}

pub fn discharge_shell(shell: u16) -> bool {
    let at = 40 * usize::from(shell) + 6;
    include_bytes!("../contacts.bin").get(at..at + 2) == Some(&111u16.to_le_bytes())
}

#[derive(Clone, Copy, Default)]
pub struct Charge {
    points: [u16; 9],
    prior: [u8; 9],
    ready: u16,
    marked: u16,
    delay: f32,
    stun_weakness_frames: f32,
    body_pickup_frames: f32,
    tail_next: u32,
    tail_left: u8,
    tail_part: u8,
}

impl Charge {
    pub fn marked(&self) -> u16 { self.marked }

    pub fn pool(species: u16, part: u8) -> Option<usize> {
        Self::profile(species, part).map(|row| row.0)
    }
    fn species_row(species: u16) -> Option<&'static [u8]> {
        let (records, _) = PARTS.as_chunks::<RECORD>();
        let i = records.binary_search_by_key(&species, |row| u16::from_le_bytes([row[0], row[1]])).ok()?;
        Some(&records[i])
    }

    pub fn apply_discharge_status(&mut self, species: u16, marked: u16) -> (bool, bool) {
        let Some(row) = Self::species_row(species) else { return (false, false); };
        let has_effect = |id| row[2..].chunks_exact(PART_SIZE).any(|p|
            p[9] == id && p[0] < 9 && marked & (1 << p[0]) != 0);
        let head = has_effect(1);
        let body = has_effect(4);
        if head { self.stun_weakness_frames = 1200.; }
        // 855b01 starts 150 Frontier ticks and clears the sharpness award flag.
        if body { self.body_pickup_frames = 300.; }
        (head, body)
    }

    pub fn stun_weakened(&self) -> bool { self.stun_weakness_frames > 0. }
    // Align to the quest clock just as 8568a0 uses the global 30-tick phase.
    pub fn arm_tail(&mut self, species: u16, mask: u16, clock: (u32, u32)) -> bool {
        let Some(row) = Self::species_row(species) else { return false; };
        let Some(part) = row[2..].chunks_exact(PART_SIZE).take(8).position(|p|
            p[9] == 5 && p[0] < 9 && mask & (1 << p[0]) != 0) else { return false; };
        self.tail_part = part as u8;
        let (now, fps) = clock;
        self.tail_next = now - now % fps;
        self.tail_left = 15;
        true
    }
    pub fn tail_active(&self) -> bool { self.tail_left > 0 }
    pub fn tail_part(&self) -> u8 { self.tail_part }
    pub fn tail_due(&mut self, (now, fps): (u32, u32)) -> bool {
        if !self.tail_active() || now > self.tail_next { return false; }
        let due = (self.tail_next - now) / fps + 1;
        let expired = due > u32::from(self.tail_left);
        self.tail_left = self.tail_left.saturating_sub(due.min(15) as u8);
        self.tail_next = self.tail_next.saturating_sub(fps.saturating_mul(due));
        !expired && now != 0
    }
    pub fn tail_damage(durability: u32) -> u32 { (durability * 600 / 1000) / 15 }
    pub fn body_pickup_available(&self) -> bool { self.body_pickup_frames > 0. }
    pub fn take_body_pickup(&mut self) -> bool {
        core::mem::take(&mut self.body_pickup_frames) > 0.
    }

    fn profile(species: u16, part: u8) -> Option<(usize, u16, u16, f32)> {
        if part >= 9 { return None; }
        let row = Self::species_row(species)?;
        let offset = 2 + usize::from(part) * PART_SIZE;
        let pool = usize::from(row[offset]);
        let first = u16::from_le_bytes([row[offset + 1], row[offset + 2]]);
        let step = u16::from_le_bytes([row[offset + 3], row[offset + 4]]);
        let factor = f32::from_le_bytes(row[offset + 5..offset + 9].try_into().ok()?);
        (pool < 9).then_some((pool, first, step, factor))
    }

    /// Frontier 8553f0 credits each raw part mapped to a discharged pool.
    pub fn part_damage(species: u16, marked: u16, part: u8, max_hp: i32) -> u32 {
        let Some((pool, _, _, factor)) = Self::profile(species, part) else { return 0; };
        if marked & (1 << pool) == 0 { return 0; }
        (max_hp as f32 * factor) as u32
    }

    pub fn hit(&mut self, species: u16, part: u8, gain: u16) -> Option<(usize, u16, u16)> {
        if gain == 0 { return None; }
        let (pool, first, step, _) = Self::profile(species, part)?;
        let points = &mut self.points[pool];
        let prior = self.prior[pool];
        let extra = if prior == 0 {
            0
        } else {
            (u32::from(step) << (prior - 1).min(15)).min(9999) as u16
        };
        let limit = first.saturating_add(extra);
        if *points >= limit { return None; }
        *points = points.saturating_add(gain).min(limit);
        if *points == limit { self.ready |= 1 << pool; }
        Some((pool, *points, limit))
    }

    pub fn discharge(&mut self, species: u16, part: u8) -> u32 {
        let Some((pool, _, _, _)) = Self::profile(species, part) else { return 0; };
        if self.marked != 0 && self.ready & (1 << pool) == 0 { return 0; }
        let new = self.ready & !self.marked;
        if new != 0 {
            if self.marked == 0 { self.delay = 2. * f32::from(ticks(0)); }
            self.marked |= new;
        }
        new.count_ones()
    }

    pub fn tick(&mut self, elapsed: f32) -> u16 {
        self.stun_weakness_frames = (self.stun_weakness_frames - elapsed).max(0.);
        self.body_pickup_frames = (self.body_pickup_frames - elapsed).max(0.);
        if self.marked == 0 { return 0; }
        self.delay -= elapsed;
        if self.delay > 0. { return 0; }
        for i in 0..self.points.len() {
            if self.marked & (1 << i) != 0 {
                self.points[i] = 0;
                self.prior[i] = self.prior[i].saturating_add(1);
            }
        }
        self.ready &= !self.marked;
        core::mem::take(&mut self.marked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_weakness_refreshes_on_head_discharge_only() {
        for species in [1, 2, 0x201, 0x202] {
            let mut charge = Charge::default();
            assert_eq!(charge.apply_discharge_status(species, 1 | (1 << 7)), (false, true));
            assert!(!charge.stun_weakened());
            assert_eq!(charge.apply_discharge_status(species, 1 << 5), (true, false));
            charge.tick(1199.);
            assert!(charge.stun_weakened());
            assert_eq!(charge.apply_discharge_status(species, 1 | (1 << 5)), (true, true));
            charge.tick(1199.);
            assert!(charge.stun_weakened());
            charge.tick(1.);
            assert!(!charge.stun_weakened());
            assert_eq!(charge.apply_discharge_status(0xffff, 1 << 5), (false, false));
        }
    }

    #[test]
    fn tail_pulses_follow_quest_seconds_and_refresh_without_stacking() {
        for species in [1, 2, 0x201, 0x202] {
            for fps in [30, 60] {
                let mut charge = Charge::default();
                assert!(!charge.arm_tail(species, 1 | (1 << 5), (615 * fps, fps)));
                assert!(charge.arm_tail(species, 1 << 7, (615 * fps + 3, fps)));
                assert!(!charge.tail_due((615 * fps + 1, fps)));
                for second in (601..=615).rev() {
                    assert!(charge.tail_due((second * fps, fps)));
                    assert!(!charge.tail_due((second * fps, fps)));
                }
                assert!(!charge.tail_active());
                charge.arm_tail(species, 1 << 7, (500 * fps, fps));
                assert!(charge.tail_due((500 * fps, fps)));
                charge.arm_tail(species, 1 << 7, (480 * fps + 5, fps));
                assert!(!charge.tail_due((464 * fps, fps)));
                assert!(!charge.tail_active());
            }
        }
        assert_eq!([24, 25, 199, 450].map(Charge::tail_damage), [0, 1, 7, 18]);
    }

    #[test]
    fn body_pickup_is_once_per_discharge_and_expires_at_five_seconds() {
        for species in [1, 2, 0x201, 0x202] {
            let mut charge = Charge::default();
            charge.apply_discharge_status(species, 1);
            charge.tick(299.);
            assert!(charge.take_body_pickup());
            assert!(!charge.take_body_pickup());
            charge.apply_discharge_status(species, 1);
            charge.tick(299.);
            charge.apply_discharge_status(species, 1 << 7);
            charge.tick(1.);
            assert!(!charge.body_pickup_available());
        }
    }

    #[test]
    fn delayed_damage_reaches_each_matching_gu_part_once() {
        for species in [1, 2, 0x201, 0x202] {
            let mut charge = Charge::default();
            for part in [0, 5, 7] { charge.hit(species, part, 400); }
            assert_eq!(charge.discharge(species, 6), 3);
            assert_eq!(charge.tick(99.), 0);
            let mask = charge.tick(1.);
            assert_eq!(mask, 1 | 1 << 5 | 1 << 7);
            assert_eq!(charge.tick(100.), 0);
            let amounts = core::array::from_fn(|i| Charge::part_damage(species, mask, i as u8, 2205));
            assert_eq!(amounts, [4, 4, 4, 4, 4, 33, 33, 110, 0]);
            assert_eq!(Charge::part_damage(species, 1 << 5, 7, 2205), 0);
        }
    }

    #[test]
    fn discharge_marks_distinct_ready_pools_and_preserves_the_first_deadline() {
        let mut charge = Charge::default();
        assert_eq!(charge.discharge(2, 0), 0);
        charge.hit(2, 0, 400);
        charge.hit(2, 5, 300);
        assert_eq!(charge.discharge(2, 6), 2);
        charge.tick(60.);
        charge.hit(2, 7, 200);
        assert_eq!(charge.discharge(2, 8), 0);
        assert_eq!(charge.discharge(2, 7), 1);
        assert_eq!(charge.discharge(2, 0), 0);
        charge.tick(40.);
        for part in [0, 5, 7] {
            assert_eq!(charge.hit(2, part, 1).unwrap().1, 1);
            charge.hit(2, part, u16::MAX);
        }
        assert_eq!(charge.discharge(2, 0), 3);
        assert_eq!([resonance_ticks(1), resonance_ticks(2), resonance_ticks(3)], [300, 600, 1200]);
    }

    #[test]
    fn gu_breaks_select_the_part_that_receives_the_discharge() {
        let mut charge = Charge::default();
        assert_eq!(charge.hit(32, 0, 10), Some((5, 10, 300)));
        assert_eq!(charge.hit(32, 3, 10), Some((7, 10, 200)));
        assert_eq!(charge.hit(32, 5, 10), Some((0, 10, 400)));
        assert!(charge.arm_tail(32, 1 << 7, (600, 60)));
        assert_eq!(charge.tail_part(), 3);
        // Breakable but attached Rajang tail, and uncuttable Gypceros tail.
        for (species, part) in [(23, 4), (9, 7)] {
            assert_eq!(Charge::default().hit(species, part, 10), Some((0, 10, 400)));
        }
        assert!(charge.arm_tail(55, 1 << 7, (600, 60)));
        assert_eq!(charge.tail_part(), 7);
        assert_eq!(charge.apply_discharge_status(30, 1), (true, false));
    }

    #[test]
    fn dromes_share_one_charge_and_burst_across_both_gu_parts() {
        for species in [14, 15, 16, 34] {
            let mut charge = Charge::default();
            assert_eq!(charge.hit(species, 1, 220), Some((0, 220, 400)));
            assert_eq!(charge.hit(species, 0, 180), Some((0, 400, 400)));
            assert_eq!(charge.hit(species, 2, 20), None);
            assert_eq!(charge.discharge(species, 1), 1);
            let mask = charge.tick(100.);
            assert_eq!(mask, 1);
            assert_eq!(Charge::part_damage(species, mask, 0, 2205), 110);
            assert_eq!(Charge::part_damage(species, mask, 1, 2205), 0);
            assert_eq!(charge.apply_discharge_status(species, mask), (true, false));
            assert!(!charge.arm_tail(species, mask, (600, 60)));
            assert_eq!(charge.hit(species, 1, 10), Some((0, 10, 800)));
        }
    }

    #[test]
    fn rathian_groups_contacts_by_part_and_caps_each_pool() {
        let mut charge = Charge::default();
        assert_eq!(shell_gain(1), 38);
        assert_eq!(charge.hit(1, 1, 38), Some((0, 38, 400)));
        assert_eq!(charge.hit(1, 4, 38), Some((0, 76, 400)));
        assert_eq!(charge.hit(1, 5, 38), Some((5, 38, 300)));
        assert_eq!(charge.hit(1, 6, 38), Some((5, 76, 300)));
        assert_eq!(charge.hit(1, 7, 999), Some((7, 200, 200)));
        assert_eq!(charge.hit(1, 7, 38), None);
        assert_eq!(charge.hit(1, 8, 38), None);
        assert_eq!(charge.hit(1, 6, shell_gain(9)), Some((5, 77, 300)));
        for shell in [12, 14, 27, 28] {
            assert_eq!(charge.hit(1, 6, shell_gain(shell)), None);
        }
        assert_eq!(charge.hit(0xffff, 6, 38), None);
        assert_eq!(charge.discharge(1, 6), 1);
        assert_eq!(charge.discharge(1, 7), 0);
        charge.tick(99.);
        assert_eq!(charge.hit(1, 7, 1), None);
        charge.tick(1.);
        assert_eq!(charge.hit(1, 7, 1), Some((7, 1, 400)));
        assert_eq!(charge.hit(1, 6, 1), Some((5, 78, 300)));
    }
}
