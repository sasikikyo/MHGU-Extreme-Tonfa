use core::sync::atomic::{AtomicU32, Ordering};

const VARIANTS: &[u8; 137] = include_bytes!("../mechanisms.bin");
const PATHS: [&core::ffi::CStr; 2] = [
    c"player\\mot\\frontier_tonfa\\mechanism0",
    c"player\\mot\\frontier_tonfa\\mechanism1",
];
static BANKS: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];

// GU passes blend, initial frame and playback rate in the VFP registers.
core::arch::global_asm!(
    ".arm", ".global tonfa_weapon_motion", "tonfa_weapon_motion:",
    "mov ip,r2", "mov r2,r1", "mov r1,#0", "mov r3,#0",
    "vmov s0,r3", "vmov s1,r3", "vmov.f32 s2,#1.0", "bx ip"
);
unsafe extern "C" { fn tonfa_weapon_motion(weapon: u32, id: u32, entry: u32); }

unsafe fn bank(weapon: u32, variant: usize) -> Option<u32> {
    let base = forge::mem::text_addr();
    unsafe {
        let mut resource = BANKS[variant].load(Ordering::Acquire);
        if resource == 0 {
            let manager = *((base + 0x2126a64) as *const u32);
            if manager == 0 { return None; }
            let vtable = *(manager as *const *const u32);
            let load: unsafe extern "C" fn(u32, u32, *const core::ffi::c_char, u32) -> u32 =
                core::mem::transmute(vtable.add(12).read());
            resource = load(manager, base + 0x2125210, PATHS[variant].as_ptr(), 1);
            if resource == 0 { return None; }
            if *((resource + 80) as *const u32) & 1 == 0 {
                let release: unsafe extern "C" fn(u32) = core::mem::transmute(base + 0x887660);
                release(resource);
                return None;
            }
            // Attachment updates for both hands run on the game thread.
            BANKS[variant].store(resource, Ordering::Release);
        }
        let mut empty = None;
        for bank in (8..16).rev() {
            let bound = *((weapon + 3844 + bank * 4) as *const u32);
            if bound == resource { return Some(bank); }
            if bound == 0 && empty.is_none() { empty = Some(bank); }
        }
        let bank = empty?;
        let bind: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(base + 0x957400);
        bind(weapon, resource, bank);
        Some(bank)
    }
}

fn destination(slot: u8) -> u8 {
    match slot { 7 | 23 => 2, 9 | 20 => 4, 13 | 15 => 1, _ => slot }
}

pub(super) unsafe fn update(weapon: u32, actor: u32, drawn: bool) {
    unsafe {
        let gear = *((actor + 5060) as *const u32);
        let family = *((gear + 30) as *const u16);
        let Some(&variant) = VARIANTS.get(usize::from(family).wrapping_sub(1)) else { return; };
        let Some(bank) = bank(weapon, usize::from(variant)) else { return; };
        let current = *((weapon + 1204) as *const u16) as u32;
        let target = if !drawn { 1 } else if crate::gauge::short(actor) { 2 } else { 4 };
        let previous = if current >> 8 == bank { current as u8 } else { 0 };
        let from = destination(previous);
        // Frontier 13140b0 waits for this hand's clip to finish before
        // consuming its pending form. Preserve both clips on a quick redraw.
        if previous != from && *((weapon + 1280) as *const f32) + 1. <
                               *((weapon + 1284) as *const f32) { return; }
        // The rows for compact, short and long in Frontier's 7x7 table at
        // 1162bb38, indexed by previous state * 7 + next state.
        let slot = match (from, target) {
            (1, 2) => 7, (1, 4) => 9, (2, 1) => 13, (4, 1) => 15,
            (2, 4) => 20, (4, 2) => 23,
            _ if previous == target => return,
            _ => target,
        };
        let id = (bank << 8) | u32::from(slot);
        tonfa_weapon_motion(weapon, id, forge::mem::text_addr() + 0x954640);
        forge::log::prelude::info!("Tonfa mechanism family {} bank {} motion {} -> {} active {}",
            family, variant, previous, slot, *((weapon + 1204) as *const u16));
    }
}
