use mhgu_sdk::style::particle::{self, Anchor, Bank};

static COMBAT: Bank = Bank::new(c"effect\\frontier_tonfa\\combat", include_bytes!("../effect-routes.bin"));
static CHARGE: particle::TrackedUnits<8> = particle::TrackedUnits::new();

pub(crate) fn bind(style: u32) {
    extern "C" fn released(unit: u32) { CHARGE.forget(unit); }
    CHARGE.bind(style, released);
}

pub(crate) unsafe fn stop_charge() { unsafe { CHARGE.stop(); } }

pub(crate) unsafe fn charge(actor: u32, stage: u8) {
    unsafe { CHARGE.stop(); }
    let Some(parent) = (unsafe { particle::parent(actor) }) else { return };
    let anchor = Anchor::Joint { parent, joint: 2, offset: None };
    // 1318160 starts one sustained layer at entry, then a flash and a
    // replacement glow at each threshold. The last glow lasts until release.
    let (flash, glow) = match stage { 0 => (None,9), 1 => (Some(10),11), _ => (Some(12),13) };
    unsafe {
        if let Some(route) = flash { COMBAT.emit(route, anchor); }
        CHARGE.emit_layers(&COMBAT, glow, anchor, u32::MAX, 0);
    }
}

pub(crate) unsafe fn flurry_punch(actor: u32, count: usize) {
    let joint=if count%2==1 {12} else {8};
    let Some(matrix)=(unsafe { particle::joint_matrix(actor,joint) }) else {return};
    let at=unsafe { ((matrix+48) as *const [f32;3]).read_unaligned() };
    let yaw=unsafe { ((actor+4076) as *const u16).read() };
    // Frontier 1316ca0 uses strict comparisons against weapon tuning +5c4..5c8.
    let tier=usize::from(count>4)+usize::from(count>8)+usize::from(count>12);
    unsafe { COMBAT.emit(5+tier,Anchor::World(at,yaw)); }
}

pub(crate) unsafe fn preload() {
    unsafe { COMBAT.preload() }
}

pub(crate) unsafe fn motion(actor: u32, clip: u16, from: f32, to: f32) {
    if clip==1213 && from<8. && to>=8. {unsafe {both_hands(actor,14);}}
    for row in include_bytes!("../effect-joints.bin").as_chunks::<14>().0 {
        let word = |i: usize| u16::from_le_bytes([row[i], row[i+1]]);
        let frame = f32::from(word(2));
        if word(0) != clip || frame <= from || frame > to { continue; }
        let Some(parent) = (unsafe { particle::parent(actor) }) else { continue };
        let offset = [8,10,12].map(|i| f32::from(word(i) as i16));
        unsafe { COMBAT.emit(usize::from(word(4)), Anchor::JointPose {
            parent, joint: word(6) as i16, offset: Some(offset),
        }); }
    }
    for cue in include_bytes!("../effect-motion.bin").as_chunks::<12>().0 {
        let motion = u16::from_le_bytes([cue[0], cue[1]]);
        if motion != clip { continue; }
        let frame = f32::from(u16::from_le_bytes([cue[2], cue[3]]));
        if frame <= from || frame > to { continue; }
        let route = usize::from(u16::from_le_bytes([cue[4], cue[5]]));
        unsafe {
            let mut position = ((actor + 64) as *const [f32; 3]).read();
            let yaw = ((actor + 4076) as *const u16).read();
            let offset = |i: usize| f32::from(i16::from_le_bytes([cue[i], cue[i+1]]));
            let (sin, cos) = libm::sincosf(f32::from(yaw)*core::f32::consts::TAU/65536.);
            position[0] += offset(6)*cos + offset(10)*sin;
            position[1] += offset(8);
            position[2] += offset(10)*cos - offset(6)*sin;
            COMBAT.emit(route, Anchor::World(position, yaw));
        }
    }
}

unsafe fn both_hands(actor: u32, route: usize) {
    let Some(parent) = (unsafe { particle::parent(actor) }) else { return };
    // Frontier shell 101/13 posts fac/30 once on each weapon. The GU
    // dual-wield skeleton attaches its two weapon grips at joints 8 and 12.
    for joint in [8, 12] {
        unsafe { COMBAT.emit(route, Anchor::Joint { parent, joint, offset: None }); }
    }
}

pub(crate) unsafe fn evade(actor: u32) {
    unsafe { both_hands(actor, 0); }
}

pub(crate) unsafe fn evade_pulse(actor: u32) {
    unsafe { both_hands(actor, 4); }
}

unsafe fn camera_yaw() -> u16 {
    // Frontier b90120 uses the camera look direction when posting the blast.
    // GU's camera manager and eye/target vectors are also used by 27f5d0.
    unsafe {
        let manager = *((forge::mem::text_addr() + 0x2126504) as *const u32);
        if manager == 0 { return 0; }
        let camera = *((manager + 84) as *const u32);
        if camera == 0 { return 0; }
        let eye = *((camera + 64) as *const [f32; 3]);
        let target = *((camera + 96) as *const [f32; 3]);
        (libm::atan2f(target[0] - eye[0], target[2] - eye[2])
            * (65536. / core::f32::consts::TAU)) as i32 as u16
    }
}

pub(crate) unsafe fn flurry_release(tier: usize, position: [f32; 3]) {
    let mut accepted = 0;
    unsafe {
        COMBAT.emit_tracked(1 + tier, Anchor::World(position, camera_yaw()), |_| accepted += 1);
    }
    forge::log::prelude::info!("Tonfa flurry effect tier {} accepted {} layers", tier + 1, accepted);
}

pub(crate) unsafe fn qi_mark(position: [f32; 3]) {
    let route = u16::from_le_bytes(*include_bytes!("../effect-qi.bin"));
    unsafe { COMBAT.emit(usize::from(route), Anchor::World(position, camera_yaw())); }
}
