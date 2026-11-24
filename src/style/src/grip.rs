use core::sync::atomic::{AtomicU32, Ordering};

const TRACKS: &[u8] = include_bytes!("../grip.bin");

fn word(offset: usize) -> u32 {
    u32::from_le_bytes(TRACKS[offset..offset + 4].try_into().unwrap())
}

fn pose(clip: u16, hand: usize, frame: f32, wielded: bool) -> ([f32; 3], [f32; 4]) {
    let (position, orientation, _) = mount(if wielded { hand } else { 2 + hand });
    let neutral = (position, orientation);
    let bank = match clip / 100 { 10 => 0, 12 => 1, 14 => 2, _ => return neutral };
    let row = if wielded {
        8 * (2 * (100 * bank + usize::from(clip % 100)) + hand)
    } else if matches!(clip, 1002 | 1003) {
        8 * (600 + 2 * usize::from(clip - 1002) + hand)
    } else { return neutral; };
    let first = word(row) as usize;
    let count = word(row + 4) as usize;
    if count == 0 || !frame.is_finite() { return neutral; }
    let time = frame.clamp(0., (count - 1) as f32);
    let index = time as usize;
    let at = |index: usize, k| f32::from_bits(word(first + 28 * index + 4 * k));
    let fraction = time - index as f32;
    let next = (index + 1).min(count - 1);
    let sign = if (3..7).map(|k| at(index, k) * at(next, k)).sum::<f32>() < 0. { -1. } else { 1. };
    let blend = |k, sign| at(index, k) + fraction * (sign * at(next, k) - at(index, k));
    let p = core::array::from_fn(|k| blend(k, 1.));
    let mut q: [f32; 4] = core::array::from_fn(|k| blend(k + 3, sign));
    let length = libm::sqrtf(q.iter().map(|v| v*v).sum());
    for v in &mut q { *v /= length; }
    (p, q)
}

fn mount(record: usize) -> ([f32; 3], [f32; 4], f32) {
    let first = TRACKS.len() - 128 + record * 32;
    let at = |k| f32::from_bits(word(first + 4 * k));
    (core::array::from_fn(at), core::array::from_fn(|k| at(k + 3)), at(7))
}

#[forge::hook(offset=0x3078b8)]
unsafe fn attach(weapon: u32, actor: u32, part: u32) -> u32 {
    let result = original!(weapon, actor, part);
    unsafe {
        if weapon == 0 || !crate::runtime::owns(actor) { return result; }
        let parts = &*((actor + 9120) as *const [u32; 2]);
        let Some(index) = parts.iter().position(|&p| p == weapon) else { return result; };
        let native_joint = *((weapon + 0x13cc) as *const u32);
        let state = *((weapon + 0x13d4) as *const u32);
        let packed = *((actor + 1204) as *const u16);
        let clip = crate::runtime::source_clip(actor, packed);
        let drawn: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(forge::mem::text_addr() + 0x2a2444);
        let frame = *((actor + 1280) as *const f32);
        let wielded = match clip {
            Some(1002) => frame >= 10.,
            Some(1003) => frame < [38., 66.][index],
            Some(_) => true,
            None => matches!(native_joint, 8 | 12) || (state != 2 && drawn(actor) != 0),
        };
        let joint = if wielded { [8, 12][index] } else { 0 };
        if native_joint != joint {
            let table = *(weapon as *const u32);
            let bind: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(*((table + 0x48) as *const u32));
            *((weapon + 0x13cc) as *mut u32) = joint;
            bind(weapon, actor, joint);
        }
        let (p, q) = pose(clip.unwrap_or(1001), index, frame, wielded);
        let scale = if wielded { 1. } else { mount(2 + index).2 };
        static LAST: [AtomicU32; 2] = [AtomicU32::new(u32::MAX), AtomicU32::new(u32::MAX)];
        if LAST[index].swap(joint, Ordering::Relaxed) != joint {
            forge::log::prelude::info!("Tonfa mount part {} joint {} native {} clip {:?} position {:?}", part,joint,native_joint,clip,p);
        }
        *((weapon + 0x40) as *mut [f32; 3]) = p;
        *((weapon + 0x50) as *mut [f32; 4]) = q;
        *((weapon + 0x60) as *mut [f32; 3]) = [scale; 3];
        crate::mechanism::update(weapon, actor, wielded);
    }
    result
}

#[forge::hook(offset=0x308b3c)]
unsafe fn host_motion(weapon: u32, actor: u32) {
    unsafe {
        if crate::runtime::owns(actor) {
            let parts = &*((actor + 9120) as *const [u32; 2]);
            if parts.contains(&weapon) { return; }
        }
    }
    original!(weapon, actor)
}

pub(super) fn install() {
    forge::install_hook!(forge::mem::text_addr(), attach);
    forge::install_hook!(forge::mem::text_addr(), host_motion);
}
