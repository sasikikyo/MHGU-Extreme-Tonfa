use crate::program::{GUARD_HOLD, GUARD_RAISE};
use core::sync::atomic::{AtomicU32, Ordering};
use mhgu_sdk::style::{
    action,
    guard::{Decision, GuardResult},
    hunter,
    rules::{self, Rules},
};

// Frontier a45730 compares against 180 degrees +/- the weapon's half-angle.
static TONFA: Rules = Rules {
    guard_half_angle: ((110_u32 * 65536 + 180) / 360) as u16,
    ..Rules::EMPTY
};
const MIN_STAMINA: f32 = 150.;
static BLOCKS: AtomicU32 = AtomicU32::new(0);
static MOTION: AtomicU32 = AtomicU32::new(0);

pub fn bind(id: u32) {
    if !rules::bind(id, &TONFA) {
        forge::log::prelude::warn!("Tonfa guard cone could not be registered");
    }
}

unsafe fn recoil_tier(actor: u32) -> u32 {
    // GU applies armour skills to the incoming power. Frontier 8f7c1a
    // selects the recoil tier after this adjustment.
    let power: unsafe extern "C" fn(u32) -> u32 =
        unsafe { core::mem::transmute(forge::mem::text_addr() + 0x2f4a2c) };
    match unsafe { power(actor) } {
        0..=14 => 0,
        15..=39 => 1,
        _ => 2,
    }
}

unsafe fn guarding(actor: u32) -> bool {
    let (group, slot) = unsafe { action::current(actor) };
    // Frontier 9ed0cc and 9e23a7 keep the guard active during recoil.
    matches!((group, slot), (32, 3..=5))
        || matches!(action::custom_id(group, slot), Some(GUARD_RAISE | GUARD_HOLD))
}

pub unsafe extern "C" fn block(actor: u32, result: *mut GuardResult) -> Decision {
    if !unsafe { guarding(actor) } {
        return Decision::Native;
    }
    if result.is_null() || unsafe { ((actor + 5088) as *const f32).read() } < MIN_STAMINA {
        return Decision::Reject;
    }
    unsafe {
        result.write(GuardResult {
            invulnerability: 0.,
            recoil: recoil_tier(actor),
            sharpness_power: 0,
        });
    }
    Decision::Precise
}

// The Recomp dispatcher repeats the weapon-class check inside 2f0670.
// The dual-blade host can reject a hit the registry accepted at collision.
#[forge::hook(offset = 0x2f0670)]
unsafe fn resolve(actor: u32, kind: u32, damage: *mut i32, special: *mut u8) -> u32 {
    let native = original!(actor, kind, damage, special);
    if native != 0 || damage.is_null() || special.is_null() {
        return native;
    }
    unsafe {
        if !guarding(actor) || !crate::runtime::active(actor)
            || ((actor + 5068) as *const u32).read() & 1 == 0
        {
            return native;
        }
        let base = forge::mem::text_addr();
        let impact = actor + 12820;
        let flags = (impact as *const u16).read();
        let angle = ((impact + 4) as *const u16).read();
        let facing = ((actor + 5280) as *const u16).read();
        let cat_skill: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(base + 0x2864e0);
        let guard_up = hunter::has_skill(actor, 171) || hunter::has_skill(actor, 317)
            || cat_skill(actor, 18) != 0;
        // Use the same exclusions, Guard Up requirement and directional
        // test as the registry on the final incoming hit descriptor.
        if ((impact + 8) as *const i16).read() < 0 || flags & 0x100 != 0
            || matches!(kind, 0x19..=0x1d | 0x3a | 0x43 | 0x44 | 0x46)
            || (flags & 0x20 != 0 && !guard_up)
            || ((actor + 5088) as *const f32).read() < MIN_STAMINA
            || (flags & 0x40 == 0
                && (facing.wrapping_sub(angle) as i16).unsigned_abs() > TONFA.guard_half_angle)
        {
            return native;
        }

        let recoil = recoil_tier(actor);
        let react: unsafe extern "C" fn(u32, u32, u32, *mut i32) -> u32 =
            core::mem::transmute(base + 0x2f4b20);
        react(actor, recoil, impact, damage);
        if !guard_up {
            let status: unsafe extern "C" fn(u32) = core::mem::transmute(base + 0x2f2fa0);
            status(actor);
        }
        special.write(0);
        if BLOCKS.fetch_add(1, Ordering::Relaxed) < 8 {
            forge::log::prelude::info!("Tonfa guard resolved tier {} with native recoil", recoil);
        }
        1
    }
}

pub fn install() {
    forge::install_hook!(forge::mem::text_addr(), resolve);
}

// Preserve the native motion request for GU checks during recovery.
pub unsafe fn recoil_clip(actor: u32, motion: u32) -> Option<u16> {
    let clip = match (unsafe { action::current(actor) }, motion) {
        ((32, 3), 5211) => 1450,
        ((32, 4 | 5), 5212 | 5213) => 1451,
        _ => return None,
    };
    MOTION.store(motion, Ordering::Relaxed);
    Some(clip)
}

pub unsafe fn recoil_motion(actor: u32, clip: u16) -> Option<u32> {
    let motion = MOTION.load(Ordering::Relaxed);
    match (unsafe { action::current(actor) }, clip, motion) {
        ((32, 3), 1450, 5211) | ((32, 4 | 5), 1451, 5212 | 5213) => Some(motion),
        _ => None,
    }
}
