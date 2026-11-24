//! Timed motion-value bonus for evading an incoming attack with Extreme Tonfa.
const DATA: &[u8] = include_bytes!("../evade-reward.bin");
const TICKS: u16 = u16::from_le_bytes([DATA[0], DATA[1]]);
const PER_MILLE: u16 = u16::from_le_bytes([DATA[2], DATA[3]]);
const RATE_COUNT: usize = DATA[6] as usize;
const ACTIONS_START: usize = 7 + 2 * RATE_COUNT;
const _: () = assert!(RATE_COUNT > 0 && ACTIONS_START < DATA.len());

// 135e8cc uses the incoming attack's base power before hunter defenses.
pub(crate) fn base_gain(power: u16) -> u32 {
    (f32::from(power) * 7.5) as u32
}

pub(crate) fn combo_rate() -> u32 {
    u32::from(u16::from_le_bytes([DATA[4], DATA[5]]))
}

pub(crate) fn ex_rate(level: u32) -> u32 {
    let p = 7 + 2 * (level as usize).min(RATE_COUNT - 1);
    u32::from(u16::from_le_bytes([DATA[p], DATA[p + 1]]))
}

#[derive(Default, Clone, Copy)]
pub(crate) struct Reward {
    window: f32,
    remaining: f32,
}

impl Reward {
    pub const EMPTY: Self = Self {window: 0., remaining: 0.};

    pub fn arm(&mut self, action: u8, frames: f32) {
        self.window = if DATA[ACTIONS_START..].contains(&action) {frames} else {0.};
    }

    pub fn tick(&mut self, elapsed: f32) -> bool {
        self.window = (self.window - elapsed).max(0.);
        let previous = self.remaining;
        self.remaining = (self.remaining - elapsed).max(0.);
        // 1317200 posts fac/31 on both hands every fourth Frontier tick.
        // Count from the most recent success, even on fractional GU updates.
        let duration = 2. * f32::from(TICKS);
        let pulse_index = |remaining| ((duration - remaining) / 8.) as u32;
        self.remaining > 0. && pulse_index(previous) < pulse_index(self.remaining)
    }

    pub fn connect(&mut self) -> bool {
        if self.window <= 0. { return false; }
        self.window = 0.;
        self.remaining = 2. * f32::from(TICKS);
        true
    }

    pub fn armed(&self) -> bool { self.window > 0. }

    pub fn power(&self, base: u16) -> u16 {
        if self.remaining <= 0. {return base;}
        // 13148e0 multiplies the motion value, then truncates it.
        (f32::from(base) * (f32::from(1000 + PER_MILLE) * 0.001)) as u16
    }
}

#[cfg(target_arch = "arm")]
mod collision {
    use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
    use mhgu_sdk::style::action;

    static MANAGER: AtomicU32 = AtomicU32::new(0);
    static HIT: AtomicU32 = AtomicU32::new(0);
    static HUNTER: AtomicU32 = AtomicU32::new(0);

    // Scope the borrow to the collision pass. 16b2a4 checks the attack's
    // target before 16b3d4 consults the hunter's invulnerability gate.
    #[forge::hook(offset=0x16b15c)]
    unsafe fn pass(manager:u32)->u32 {
        let previous=(MANAGER.swap(manager,Relaxed),HIT.swap(0,Relaxed),HUNTER.swap(0,Relaxed));
        let result=original!(manager);
        HUNTER.store(previous.2,Relaxed);
        HIT.store(previous.1,Relaxed);
        MANAGER.store(previous.0,Relaxed);
        result
    }

    #[forge::hook(offset=0x169878)]
    unsafe fn candidate(hit:u32,actor:u32)->u32 {
        let accepted=original!(hit,actor);
        if MANAGER.load(Relaxed)!=0 {
            HUNTER.store(0,Relaxed);
            if accepted!=0 && actor!=0 && crate::gauge::evading(actor) {
                HIT.store(hit,Relaxed);
                HUNTER.store(actor,Relaxed);
            }
        }
        accepted
    }

    #[repr(C, align(16))]
    #[derive(Default)]
    struct Shape([f32; 12]);

    // GU collision slots pack two xyz vectors without padding. Its sphere,
    // capsule and swept-sphere routines take aligned local descriptors.
    unsafe fn shape(slot:u32, sphere:bool)->Shape {
        let mut out=Shape::default();
        unsafe {
            let src=(slot+8) as *const [f32;3];
            out.0[..3].copy_from_slice(&src.read_unaligned());
            if !sphere {out.0[4..7].copy_from_slice(&src.add(1).read_unaligned());}
            out.0[if sphere {3} else {8}]=((slot+4) as *const f32).read();
        }
        out
    }

    unsafe fn intersects(base:u32, manager:u32, actor:u32, hit:u32)->bool {
        unsafe {
            let index=*((actor+0x3260) as *const u32);
            if index>=256 {return false;}
            let hurt=manager+0x1c+32*index;
            if *(hurt as *const u8)!=2 {return false;}
            let capsule=shape(hurt,false);
            let resource=*((hit+0x84) as *const u32);
            if resource==0 {return false;}
            let vtable=*(resource as *const u32);
            let group:unsafe extern "C" fn(u32,u32)->u32=core::mem::transmute(*((vtable+0x44) as *const u32));
            let data=group(resource,*((hit+0x88) as *const u32));
            if data==0 {return false;}
            let first=*((hit+0x1c) as *const u32);
            let count=(*((data+4) as *const u32)).min(64u32.saturating_sub(first));
            let parts=*((data+8) as *const *const u32);
            if parts.is_null(){return false;}
            for i in 0..count {
                if *parts.add(i as usize)==0 {continue;}
                let slot=manager+0x2030+32*(first+i);
                let kind=*(slot as *const u8);
                let offset=match kind {1=>0x168198,2=>0x167b5c,3=>0x1683dc,_=>continue};
                let check:unsafe extern "C" fn(&Shape,&Shape,&mut Shape)->u32=core::mem::transmute(base+offset);
                if check(&shape(slot,kind==1),&capsule,&mut Shape::default())!=0 {return true;}
            }
            false
        }
    }

    unsafe fn registered_monster_attack(base: u32, hit: u32) -> bool {
        unsafe {
            let enemies = *((base + 0x1851fe0) as *const u32);
            let source = *((hit + 0x0c) as *const u32);
            if enemies == 0 || source == 0 { return false; }
            let slots = &*((enemies + 28) as *const [u32; 10]);
            if slots.contains(&source) { return true; }

            // GU's shell attack builder (4a2790) records the projectile as
            // source kind 2 and snapshots its owner at +70, kind 1 for enemies.
            if *((hit + 0x10) as *const u8) != 2 || *((hit + 0x6e) as *const u8) != 1 {
                return false;
            }
            let owner = *((hit + 0x70) as *const u32);
            if owner == 0 || !slots.contains(&owner) { return false; }
            // A shell can outlive its firing actor. Compare the current owner
            // without dereferencing it, using the parameter layout of 4a1b6c.
            let params = *((source + 0x136c) as *const u32);
            params != 0 && *((params + 12) as *const u32) == owner
        }
    }

    #[forge::hook(offset=0x2a680c)]
    unsafe fn invulnerable(actor:u32)->u32 {
        let result=original!(actor);
        if result==0 || actor==0 || HUNTER.compare_exchange(actor,0,Relaxed,Relaxed).is_err() {
            return result;
        }
        unsafe {
            let base=forge::mem::text_addr();
            let manager=MANAGER.load(Relaxed);
            let hit=HIT.load(Relaxed);
            if manager!=0 && hit!=0 && *((actor+0x274a) as *const u8)&1==0
                && *((actor+11072) as *const f32)>0. && *((hit+0x38) as *const i16)>0 {
                let (group,slot)=action::current(actor);
                if action::custom(group,slot) && registered_monster_attack(base,hit)
                    && intersects(base,manager,actor,hit) {
                    let projectile = *((hit + 0x10) as *const u8) == 2;
                    crate::gauge::reward_evade(actor,slot, *((hit+0x38) as *const u16), projectile);
                }
            }
        }
        result
    }
    pub(super) fn install() {
        forge::install_hook!(forge::mem::text_addr(),pass);
        forge::install_hook!(forge::mem::text_addr(),candidate);
        forge::install_hook!(forge::mem::text_addr(),invulnerable);
    }
}

#[cfg(target_arch = "arm")]
pub(crate) fn install() { collision::install(); }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_pulses_follow_the_buff_and_restart_on_refresh() {
        let mut buff = Reward::EMPTY;
        assert!(!buff.tick(20.));
        buff.arm(77, 14.);
        assert!(buff.connect());
        assert!(!buff.tick(7.5));
        assert!(buff.tick(0.5));
        assert!(!buff.tick(0.));
        assert!(buff.tick(16.));
        buff.arm(77, 14.);
        assert!(buff.connect());
        assert!(!buff.tick(7.));
        assert!(buff.tick(1.));
        assert!(!buff.tick(892.));
        assert!(!buff.tick(8.));
    }

    #[test]
    fn evades_use_incoming_power_and_the_evade_rate_column() {
        assert_eq!((base_gain(0), base_gain(1), base_gain(21)), (0, 7, 157));
        assert_eq!(combo_rate(), 1000);
        for level in 0..=7 {
            assert_eq!(ex_rate(level), 1000);
        }
    }

    #[test]
    fn eligible_dodge_needs_contact_and_refreshes_the_bonus() {
        let mut buff = Reward::EMPTY;
        for (id, frames) in [(204, 14.), (40, 14.), (77, 0.)] {
            buff.arm(id, frames);
            assert!(!buff.connect());
        }
        buff.arm(77, 14.);
        assert!(buff.armed());
        assert_eq!(buff.power(100), 100);
        buff.tick(14.);
        assert!(!buff.connect());
        buff.arm(149, 14.);
        assert!(buff.connect());
        assert!(!buff.connect());
        assert_eq!(buff.power(23), 24);
        buff.tick(899.);
        buff.arm(6, 14.);
        assert!(buff.connect());
        buff.tick(899.);
        assert_eq!(buff.power(100), 105);
        buff.tick(1.);
        assert_eq!(buff.power(100), 100);
    }
}
