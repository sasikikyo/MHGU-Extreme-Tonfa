//! Frontier's delayed flurry release (shell class 101, numbers 8 and 10).
const TABLE: &[u8] = include_bytes!("../flurry-release.bin");
const fn word(i: usize) -> u16 {
    u16::from_le_bytes([TABLE[2 * i], TABLE[2 * i + 1]])
}
const MINIMUM: u16 = word(0);
const HEADER_WORDS: usize = 3;
const _: () = assert!(TABLE.len() % 2 == 0 && TABLE.len() / 2 > HEADER_WORDS + word(2) as usize);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Target {
    actor: u32,
    state: u32,
    slot: usize,
}

#[derive(Clone, Copy, Default)]
struct Mark {
    target: Target,
    hits: u16,
    released: bool,
}

#[derive(Default)]
struct Chain([Mark; 2]);

impl Chain {
    fn contact(&mut self, shell: u16, target: Target) -> Option<u16> {
        if (79..=90).contains(&shell) {
            let index = self.0.iter().position(|m| m.target == target)
                .or_else(|| self.0.iter().position(|m| m.hits == 0))?;
            let mark = &mut self.0[index];
            mark.target = target;
            mark.hits = mark.hits.saturating_add(1);
        } else if matches!(shell, 91 | 114) {
            let mark = self.0.iter_mut().find(|m| m.target == target)?;
            if mark.hits >= MINIMUM && !mark.released {
                mark.released = true;
                return Some(mark.hits);
            }
        }
        None
    }
}

fn release_damage(hits: u16, attack: i32) -> u32 {
    let row = usize::from(hits).min(TABLE.len() / 2 - HEADER_WORDS - 1);
    (f32::from(word(HEADER_WORDS + row)) * (attack as f32 * 0.01) + 0.5) as u32
}

fn sound_cue(hits: u16) -> u8 {
    // Shell 101/9 at 11e72ae selects local cues 24, 25 and 26.
    24 + u8::from(hits >= word(1)) + u8::from(hits >= word(2))
}

#[cfg(target_arch = "arm")]
pub(crate) use game::*;

#[cfg(target_arch = "arm")]
mod game {
    use super::*;
    use mhgu_sdk::{monster::{Monster, MonsterState}, style::action::melee::BLADE_CONTACT_OFFSETS, sync::Shared};

    #[derive(Clone, Copy)]
    struct Blast {
        target: Target,
        hits: u16,
        part: u8,
        joint: u16,
        hand: usize,
        remaining: f32,
    }

    #[derive(Default)]
    struct Release {
        owner: u32,
        hands: [u16; 2],
        hand_qi: [u16; 2],
        charges: [(Target, crate::qi::Charge); 10],
        chain: Chain,
        pending: [Option<Blast>; 2],
    }
    static STATE: Shared<Option<Release>> = Shared::new(None);

    pub fn begin(actor: u32, action: u8) {
        STATE.with(|state| {
            let run = state.get_or_insert_with(Release::default);
            if run.owner != actor {
                *run = Release { owner: actor, ..Release::default() };
            }
            run.hands = [0; 2];
            run.hand_qi = [0; 2];
            if !matches!(action, 58 | 125 | 126) {
                run.chain = Chain::default();
            }
        });
    }

    pub fn submitted(hand: usize, shell: u16, qi: u16) {
        STATE.with(|state| {
            if let Some(run) = state {
                run.hands[hand] = shell;
                run.hand_qi[hand] = qi;
            }
        });
    }

    // GU 5ceb4 reads these ten sEnemy slots. Check membership again when
    // the timer expires, before borrowing a target across game updates.
    unsafe fn monsters() -> Option<[u32; 10]> {
        let manager = unsafe { *((forge::mem::text_addr() + 0x1851fe0) as *const u32) };
        if manager == 0 { return None; }
        Some(unsafe { ((manager + 0x1c) as *const [u32; 10]).read() })
    }

    pub unsafe fn contact(actor: u32, hit: u32, monster: u32, part: u32) {
        let Some(hand) = BLADE_CONTACT_OFFSETS.iter().position(|&off| hit.wrapping_sub(actor) == off)
            else { return };
        let mut resonance = 0;
        let mut marks = None;
        STATE.with(|state| {
            let Some(run) = state.as_mut().filter(|s| s.owner == actor) else { return };
            let shell = run.hands[hand];
            if shell == 0 || part == 0 { return; }
            let Some(slot) = (unsafe { monsters() }).and_then(|list| list.iter().position(|&m| m == monster))
                else { return };
            let Some(enemy) = (unsafe { Monster::from_raw(monster) }) else { return };
            let Some(record) = enemy.state() else { return };
            if record.hp() <= 0 { return; }
            let target = Target {actor: monster, state: record.as_raw(), slot};
            let raw_part = unsafe { *((part + 8) as *const u8) };
            let (owner, charge) = &mut run.charges[slot];
            if *owner != target {
                *owner = target;
                *charge = crate::qi::Charge::default();
            }
            if let Some(species) = enemy.species() {
                if crate::qi::discharge_shell(shell) {
                    let before = charge.marked();
                    resonance = charge.discharge(species.packed(), raw_part);
                    if resonance > 0 {
                        marks = Some((species.packed(), charge.marked() & !before));
                    }
                } else if let Some((pool, points, limit)) = charge.hit(species.packed(), raw_part, run.hand_qi[hand])
                    && (points == limit || points == run.hand_qi[hand]) {
                    forge::log::prelude::info!("Tonfa Dragon Qi species {} slot {} part {} pool {}: {}/{}",species.packed(),slot,raw_part,pool,points,limit);
                }
            }
            if !matches!(shell,79..=91 | 114) { return; }
            if let Some(hits) = run.chain.contact(shell, target)
                && let Some(out) = run.pending.iter_mut().find(|p| p.is_none()) {
                let joint = unsafe { (part as *const u16).read_unaligned() };
                let part = raw_part;
                // 11e699e sets 36 Frontier logic ticks; GU runs at 60 Hz.
                *out = Some(Blast { target, hits, part, joint, hand, remaining: 72.0 });
                forge::log::prelude::info!("Tonfa flurry marked slot {} part {} with {} contacts",slot,part,hits);
            }
        });
        if resonance > 0 {
            crate::gauge::resonate(actor, crate::qi::resonance_ticks(resonance));
        }
        if let Some((species, mut pools)) = marks {
            let body: unsafe extern "C" fn(u32) -> *const u8 =
                unsafe { core::mem::transmute(forge::mem::text_addr() + 0x710b0) };
            let first = unsafe { body(monster) };
            // Prefer the struck volume, then locate other newly marked pools on
            // the same living monster. Multiple volumes can share one Qi pool.
            for index in 0..=256 {
                if pools == 0 || (index > 0 && first.is_null()) { break; }
                let row = if index == 0 { part as *const u8 } else { unsafe {first.add(44*(index-1))} };
                let joint = unsafe { row.cast::<u16>().read_unaligned() };
                if joint == u16::MAX { break; }
                let Some(pool) = crate::qi::Charge::pool(species, unsafe { *row.add(8) }) else { continue };
                if pools & (1 << pool) != 0 && let Some(pos) = unsafe { joint_position(monster, joint) } {
                    unsafe { crate::effects::qi_mark(pos); }
                    pools &= !(1 << pool);
                }
            }
        }
    }

    pub fn tick(actor: u32, elapsed: f32) {
        let (ready, discharges) = STATE.with(|state| {
            if actor == 0 || state.as_ref().is_some_and(|s| s.owner != actor) {
                *state = None;
            }
            let Some(run) = state else { return ([None; 2], [None; 10]) };
            let discharges = run.charges.each_mut().map(|(target, charge)| {
                let mask = charge.tick(elapsed);
                (mask != 0 || charge.body_pickup_available() || charge.tail_active()).then_some((*target, mask))
            });
            let blasts = run.pending.each_mut().map(|pending| {
                let blast = pending.as_mut()?;
                blast.remaining -= elapsed;
                if blast.remaining <= 0.0 { pending.take() } else { None }
            });
            (blasts, discharges)
        });
        for (target, mask) in discharges.into_iter().flatten() {
            if mask != 0 { unsafe { discharge_parts(actor, target, mask); } }
            unsafe { collect_body_reward(actor, target); }
            unsafe { tick_tail(actor, target); }
        }
        for blast in ready.into_iter().flatten() { unsafe { detonate(actor, blast) }; }
    }

    // GU passes damage to the monster's virtual HP receiver in VFP s0.
    core::arch::global_asm!(
        ".syntax unified", ".arm", ".fpu vfpv3", ".text", ".align 2",
        ".hidden tonfa_blast_hp", ".global tonfa_blast_hp", ".type tonfa_blast_hp,%function",
        "tonfa_blast_hp:", "vmov s0,r2", "bx r3"
    );
    unsafe extern "C" { fn tonfa_blast_hp(monster: u32, hit: u32, amount: u32, receiver: u32); }

    // GU returns the part multiplier in VFP s0; the plugin uses soft-float ABI.
    core::arch::global_asm!(
        ".arm", ".align 2", ".hidden tonfa_part_factor", ".global tonfa_part_factor",
        ".type tonfa_part_factor,%function", "tonfa_part_factor:",
        "push {r4, lr}", "blx r2", "vmov r0, s0", "pop {r4, pc}", options(raw)
    );
    unsafe extern "C" { fn tonfa_part_factor(monster: u32, part: u32, method: u32) -> u32; }

    unsafe fn quest_clock() -> Option<(u32, u32)> {
        let base = forge::mem::text_addr();
        unsafe {
            let quest = *((base + 0x188ad90) as *const u32);
            let game = *((base + 0x2126764) as *const u32);
            if quest == 0 || game == 0 { return None; }
            let fps = *((game + 56) as *const f32) as u32;
            let frames = *((quest + 0x1c) as *const u32);
            (fps > 0 && frames > 0).then_some((frames, fps))
        }
    }

    unsafe fn tick_tail(actor: u32, target: Target) {
        let Some(clock) = (unsafe { quest_clock() }) else { return; };
        let part = STATE.with(|s| s.as_mut().and_then(|run| {
            let (saved, charge) = &mut run.charges[target.slot];
            (*saved == target && charge.tail_due(clock)).then(|| charge.tail_part())
        }));
        let Some(part) = part else { return; };
        let Some((_, state)) = (unsafe { live_target(actor, target) }) else { return; };
        if state.tail_severed() { return; }
        unsafe {
            let monster = target.actor;
            let tuning = *((monster + 0x75ec) as *const u32);
            if tuning == 0 { return; }
            let rows = *((tuning + 0x68) as *const u32);
            if rows == 0 { return; }
            let row = rows + 10 * u32::from(part);
            // Read the full durability using GU's 99db8 calculation. Calling
            // 99db8 itself would restore the monster's partially damaged part.
            let rank: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(forge::mem::text_addr() + 0xbaaac);
            let rank = rank(monster, 2);
            if rank == 0 { return; }
            let vtable = *(monster as *const u32);
            let factor = |offset| f32::from_bits(tonfa_part_factor(monster, u32::from(part), *((vtable + offset) as *const u32)));
            let scale = *((rank + 12) as *const f32) * factor(0x34cu32);
            let mut durability = scale * f32::from(*(row as *const u16));
            if *((monster + 0xb720) as *const u8) != 0 { durability *= factor(0x17cu32); }
            let amount = crate::qi::Charge::tail_damage(durability.clamp(0., 32767.) as u32);
            if amount == 0 { return; }
            let applied = part_damage(monster, part, amount);
            // Native hits set this before queuing damage (171418). Without
            // it, 98f44 can skip the selected reaction even at zero durability.
            // No blade contact ID is consumed by this delayed status pulse.
            if applied > 0 { ((state.as_raw() + 0x444) as *mut u8).write(1); }
            // Native hunter hits submit the ordinary queue and a separate
            // typed queue. Both scale the original amount independently.
            let cut: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32) =
                core::mem::transmute(forge::mem::text_addr() + 0x9d8b4);
            cut(monster, u32::from(part), 1, amount, 0, 0);
            forge::log::prelude::info!("Tonfa tail pulse: slot {} clock {} durability {} base {} applied {}",
                target.slot, clock.0, durability as u32, amount, applied);
        }
    }

    unsafe fn joint_position(target: u32, joint: u16) -> Option<[f32; 3]> {
        if joint >= 256 || unsafe { *((target + 1172) as *const u32) } == 0
            || unsafe { *((target + 1176) as *const u32) } == 0 { return None; }
        let matrix: unsafe extern "C" fn(u32, u32) -> u32 =
            unsafe { core::mem::transmute(forge::mem::text_addr() + 0xc1628) };
        let transform = unsafe { matrix(target, u32::from(joint)) };
        (transform != 0).then(|| unsafe { ((transform + 48) as *const [f32; 3]).read_unaligned() })
    }

    unsafe fn sound_position(target: u32, joint: u16) -> [f32; 3] {
        unsafe { joint_position(target, joint) }
            .unwrap_or_else(|| unsafe { ((target + 64) as *const [f32; 3]).read_unaligned() })
    }

    unsafe fn live_target<'a>(actor: u32, saved: Target) -> Option<(Monster<'a>, MonsterState<'a>)> {
        let Target { actor: target, state, slot } = saved;
        if unsafe { monsters()? }[slot] != target { return None; }
        let monster = unsafe { Monster::from_raw(target)? };
        let record = monster.state()?;
        if record.as_raw() != state || record.hp() <= 0 { return None; }
        unsafe {
            if *((target + 0xb5f7) as *const u8) != 0
                || *((target + 0x1054) as *const u8) != *((actor + 0x1054) as *const u8) { return None; }
        }
        Some((monster, record))
    }

    unsafe fn part_damage(target: u32, part: u8, damage: u32) -> i32 {
        let apply: unsafe extern "C" fn(u32, u32, u32, u32) -> i32 =
            unsafe { core::mem::transmute(forge::mem::text_addr() + 0x9d6f8) };
        // Mode 0 adds ordinary part damage. Mode 2 forces a 10,000-point break.
        unsafe { apply(target, u32::from(part), damage, 0) }
    }

    unsafe fn discharge_parts(actor: u32, target: Target, mask: u16) {
        let Some((monster, record)) = (unsafe { live_target(actor, target) }) else { return };
        let Some(species) = monster.species() else { return };
        let max_hp = record.max_hp();
        // GU has eight ordinary part accumulators. Index 8 is a species
        // callback and has no matching Rathian or Rathalos hurt volume.
        for part in 0..8 {
            let amount = crate::qi::Charge::part_damage(species.packed(), mask, part, max_hp);
            if amount == 0 { continue; }
            let applied = unsafe { part_damage(target.actor, part, amount) };
            forge::log::prelude::info!("Tonfa Qi discharge slot {} part {}: max HP {}, base {}, applied {}",target.slot,part,max_hp,amount,applied);
        }
        STATE.with(|state| {
            if let Some(run) = state.as_mut() {
                let (saved, charge) = &mut run.charges[target.slot];
                if *saved == target {
                    let (head, body) = charge.apply_discharge_status(species.packed(), mask);
                    if head { forge::log::prelude::info!("Tonfa head discharge: slot {} stun weakness for 1200 frames",target.slot); }
                    if body { forge::log::prelude::info!("Tonfa body discharge: slot {} pickup for 300 frames",target.slot); }
                    if !record.tail_severed() && let Some(clock) = unsafe { quest_clock() }
                        && charge.arm_tail(species.packed(), mask, clock) {
                        forge::log::prelude::info!("Tonfa tail discharge: slot {} clock {} rate {}",target.slot,clock.0,clock.1);
                    }
                }
            }
        });
    }

    // Frontier 856290 checks horizontal distance to the current body joints.
    unsafe fn inside_body_aura(actor: u32, monster: u32) -> bool {
        let body: unsafe extern "C" fn(u32) -> *const u8 =
            unsafe { core::mem::transmute(forge::mem::text_addr() + 0x710b0) };
        let mut row = unsafe { body(monster) };
        if row.is_null() { return false; }
        let pos = unsafe { *((actor + 64) as *const [f32; 3]) };
        for _ in 0..256 {
            let joint = unsafe { row.cast::<u16>().read_unaligned() };
            if joint == u16::MAX { break; }
            if joint < 255 && let Some(at) = unsafe { joint_position(monster, joint) } {
                let dx = pos[0] - at[0];
                let dz = pos[2] - at[2];
                if dx * dx + dz * dz < 350. * 350. { return true; }
            }
            row = unsafe { row.add(44) };
        }
        false
    }

    unsafe fn collect_body_reward(actor: u32, target: Target) {
        let pending = STATE.with(|s| s.as_ref().is_some_and(|run|
            run.charges[target.slot].1.body_pickup_available()));
        if !pending || unsafe { live_target(actor, target) }.is_none()
            || !unsafe { inside_body_aura(actor, target.actor) } { return; }
        let taken = STATE.with(|s| s.as_mut().is_some_and(|run| {
            let (saved, charge) = &mut run.charges[target.slot];
            *saved == target && charge.take_body_pickup()
        }));
        if !taken { return; }
        crate::gauge::grant_body_attack(actor);
        let sharpen: unsafe extern "C" fn(u32, i32, u32) =
            unsafe { core::mem::transmute(forge::mem::text_addr() + 0x2ad288) };
        let sharpness = (actor + 0x2774) as *const i32;
        let before = unsafe { sharpness.read() };
        unsafe { sharpen(actor, 15, 1); }
        forge::log::prelude::info!("Tonfa body reward: slot {} sharpness {} -> {}, attack +25 for 1800 frames",
            target.slot, before, unsafe { sharpness.read() });
    }

    struct StunSample {
        target: Target,
        owner: u32,
        raw_part: u8,
        pending_part: u16,
        pending_stun: u16,
    }

    unsafe fn sample_weakened_hit(hit: u32, monster: u32, part: u32) -> Option<StunSample> {
        if hit == 0 || monster == 0 || part == 0 || unsafe { *((hit + 0x6d) as *const u8) } != 1 {
            return None;
        }
        let (target, owner) = STATE.with(|s| {
            let run = s.as_ref()?;
            let (target, _) = run.charges.iter().find(|(t, c)| t.actor == monster && c.stun_weakened())?;
            Some((*target, run.owner))
        })?;
        unsafe {
            live_target(owner, target)?;
            let raw_part = *((part + 8) as *const u8);
            if raw_part >= 8 { return None; }
            Some(StunSample {
                target, owner, raw_part,
                pending_part: *((target.state + 0x488 + 2 * u32::from(raw_part)) as *const u16),
                pending_stun: *((target.state + 0x4d2) as *const u16),
            })
        }
    }

    // Wrap GU's hit dispatcher so both zero base KO (which skips 9db54)
    // and zero hitzone KO can be distinguished from ordinary positive KO.
    #[forge::hook(offset = 0)]
    unsafe fn hunter_hit(manager: u32, hit: u32, monster: u32, point: u32,
                         a5: u32, a6: u32, part: u32, a8: u32) -> u32 {
        let sample = unsafe { sample_weakened_hit(hit, monster, part) };
        let result = original!(manager, hit, monster, point, a5, a6, part, a8);
        if let Some(s) = sample {
            unsafe {
                if live_target(s.owner, s.target).is_some() {
                    let state = s.target.state;
                    let applied = *((state + 0x488 + 2 * u32::from(s.raw_part)) as *const u16);
                    let stun = (state + 0x4d2) as *mut u16;
                    if applied != s.pending_part && stun.read() == s.pending_stun {
                        stun.write(s.pending_stun.wrapping_add(1));
                        forge::log::prelude::info!("Tonfa head weakness: slot {} part {} gained 1 KO",s.target.slot,s.raw_part);
                    }
                }
            }
        }
        result
    }

    pub fn install() {
        // Follow our equipment module's two jumps to its private, relocatable
        // observer. An older equipment module can continue playing without it.
        let site = forge::mem::text_addr() + 0x16c938;
        unsafe {
            let read = |addr: u32| (addr as *const u32).read();
            let jump = read(site);
            if jump & 0xffffff00 != 0xe28ff700 { return; }
            let window = site + 8 + ((jump & 255) << 18);
            let branch = read(window);
            if branch >> 24 != 0xea { return; }
            let stub = (window + 8).wrapping_add(((branch << 8) as i32 >> 6) as u32);
            if read(stub) != 0xe59fe004 || read(stub + 4) != 0xe79fe00e { return; }
            forge::install_hook!(stub + 16, hunter_hit);
        }
    }

    unsafe fn detonate(actor: u32, blast: Blast) {
        let Target {actor: target, slot, ..} = blast.target;
        if (unsafe { live_target(actor, blast.target) }).is_none() { return; }
        unsafe {
            let gear = *((actor + 5060) as *const u32);
            if gear == 0 { return; }
            let raw = i32::from(*((gear + 800) as *const i16));
            let attack = crate::gauge::combo_power(actor, raw);
            let damage = release_damage(blast.hits, attack);
            if damage == 0 { return; }
            let position = sound_position(target, blast.joint);
            let cue = sound_cue(blast.hits);
            crate::battle::release_sound(actor, cue, position);
            crate::effects::flurry_release(usize::from(cue - 24), position);
            // The virtual callback is optional per monster. GU 97ddc also
            // consumes the part accumulator for HP, flinches and breaks.
            let vtable = *(target as *const u32);
            let receiver = *((vtable + 0x2f8) as *const u32);
            let hit = actor + BLADE_CONTACT_OFFSETS[blast.hand];
            if receiver != 0 {
                tonfa_blast_hp(target, hit, (damage as f32).to_bits(), receiver);
            }
            part_damage(target, blast.part, damage);
            forge::log::prelude::info!("Tonfa flurry blast slot {} part {}: {} contacts, attack {}, damage {}",slot,blast.part,blast.hits,attack,damage);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_monsters_build_separately_and_each_releases_once() {
        let a = Target {actor: 10, state: 20, slot: 0};
        let b = Target {actor: 11, state: 21, slot: 1};
        let mut chain = Chain::default();
        assert_eq!(chain.contact(91, a), None);
        for _ in 0..12 { chain.contact(79, a); }
        for _ in 0..3 { chain.contact(90, b); }
        assert_eq!(chain.contact(114, b), Some(3));
        assert_eq!(chain.contact(91, a), Some(12));
        assert_eq!(chain.contact(91, b), None);
        assert_eq!(chain.contact(114, a), None);
        assert_eq!([0, 1, 3, 4, 11, 12, 99].map(|n| release_damage(n, 335)),
                   [0, 168, 168, 335, 335, 670, 670]);
        assert_eq!([1,3,4,11,12,99].map(sound_cue), [24,24,25,25,26,26]);
    }
}
