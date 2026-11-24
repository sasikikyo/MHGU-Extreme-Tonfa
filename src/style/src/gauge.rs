use core::sync::atomic::{AtomicU32, Ordering};
use mhgu_sdk::{style::{action, physical}, sync::Shared};

const TUNING: &[u8] = include_bytes!("../gauge-tuning.bin");
const fn param(i: usize) -> u32 {
    u16::from_le_bytes([TUNING[i*2], TUNING[i*2+1]]) as u32
}
const LEVEL: u32 = param(0);
const LEVELS: u32 = param(1);
const ATTACK_RATES: usize = 14;
const _: () = assert!(TUNING.len() == (ATTACK_RATES + LEVELS as usize) * 2);

static COMBO_POWER: physical::Rule = physical::Rule {
    scope: physical::Scope::BladeSlots,
    adjust: combo_power,
};
static LAST_REPORTED_LEVEL: AtomicU32 = AtomicU32::new(u32::MAX);

pub fn bind(style_id: u32) {
    if !unsafe { physical::bind(style_id, &COMBO_POWER) } {
        forge::log::prelude::warn!("Tonfa combo attack rule could not be registered");
    }
}

pub(crate) unsafe extern "C" fn combo_power(actor: u32, raw: i32) -> i32 {
    let Some((level, body_bonus)) = METER.with(|m|
        (m.owner == actor).then_some((m.combo / LEVEL, m.body_attack_frames > 0.))) else {
        return raw;
    };
    // a6ea32 adds the body reward before 11e5800 scales Tonfa combo attack.
    let base = if body_bonus { raw + 25 } else { raw };
    // 11e5800 looks up completed levels only. It converts the percentage
    // to a float multiplier before truncating the resulting physical attack.
    let adjusted = if level == 0 {
        base
    } else {
        let percent = param(ATTACK_RATES + level as usize - 1);
        (base as f32 * (percent as f32 * 0.01)) as i32
    };
    let report = level | ((body_bonus as u32) << 16);
    if LAST_REPORTED_LEVEL.swap(report, Ordering::Relaxed) != report {
        forge::log::prelude::info!("Tonfa combo level {}: physical attack {} -> {}", level, raw, adjusted);
    }
    adjusted
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Meter {
    owner: u32,
    combo: u32,
    ex: u32,
    remaining: f32,
    timeout: f32,
    evade: crate::evade::Reward,
    short: bool,
    holding_resonance: bool,
    resonance_frames: f32,
    resonance_duration: f32,
    body_attack_frames: f32,
}
static METER: Shared<Meter> = Shared::new(Meter {
    owner:0, short:false, combo:0, ex:0, remaining:0., timeout:0., evade:crate::evade::Reward::EMPTY,
    holding_resonance:false, resonance_frames:0., resonance_duration:0., body_attack_frames:0.,
});

impl Meter {
    fn add_combo(&mut self, points: u32) {
        let level = self.combo / LEVEL;
        self.timeout = ((param(2) * (LEVELS - level) + param(3) * level) / LEVELS) as f32;
        self.remaining = self.timeout;
        self.combo = (self.combo + points).min(LEVEL * LEVELS);
    }
}

pub fn tick(owner:u32, elapsed:f32) {
    let pulse = METER.with(|m| {
        if owner!=m.owner || owner==0 {
            *m=Meter{owner,..Meter::default()};
            return false;
        }
        // MHF's timeout is authored at 30 Hz, GU elapsed is in 60-Hz frames.
        m.remaining=(m.remaining-elapsed*0.5).max(0.);
        let pulse = m.evade.tick(elapsed);
        m.body_attack_frames = (m.body_attack_frames - elapsed).max(0.);
        if m.resonance_frames > 0. {
            // 1306b1d holds a newly awarded timer through the discharge pose.
            if m.holding_resonance {
                let (group, slot) = unsafe { action::current(owner) };
                m.holding_resonance = matches!(action::custom_id(group, slot), Some(5|9|13|62|66|86));
            }
            m.add_combo(LEVEL * LEVELS);
            m.ex = LEVEL * LEVELS;
            if !m.holding_resonance { m.resonance_frames = (m.resonance_frames - elapsed).max(0.); }
        }
        if m.remaining==0. { m.combo=0; }
        pulse
    });
    if pulse { unsafe { crate::effects::evade_pulse(owner); } }
}
pub fn reach(short:bool) { METER.with(|m| m.short=short); }
pub fn short(actor: u32) -> bool { METER.with(|m| m.owner == actor && m.short) }
pub fn grant_body_attack(actor: u32) {
    METER.with(|m| if m.owner == actor { m.body_attack_frames = 1800.; });
}
pub fn evade_window(action:u8,frames:f32) { METER.with(|m| m.evade.arm(action,frames)); }
pub fn motion_power(actor:u32,base:u16)->u16 {
    METER.with(|m| if m.owner==actor {m.evade.power(base)} else {base})
}
pub fn can_spend()->bool { METER.with(|m| m.ex>=LEVEL) }
pub fn resonate(owner:u32, ticks:u16) {
    METER.with(|m| {
        if owner == 0 || owner != m.owner { return; }
        m.resonance_frames = (m.resonance_frames + 2. * f32::from(ticks)).min(2. * f32::from(u16::MAX));
        m.resonance_duration = m.resonance_frames;
        m.holding_resonance = true;
        m.add_combo(LEVEL * LEVELS);
        m.ex = LEVEL * LEVELS;
        forge::log::prelude::info!("Tonfa discharge granted {} Frontier ticks of resonance",ticks);
    });
}
pub fn spend(){METER.with(|m| if m.resonance_frames == 0. { m.ex=m.ex.saturating_sub(LEVEL); });}
pub fn flurry_cost() -> bool {
    // ZZ weapon parameter +0x5bc is in raw points, unlike event 18.
    METER.with(|m|{
        if m.resonance_frames > 0. { return true; }
        m.ex=m.ex.saturating_sub(param(13));
        m.ex>0
    })
}

pub fn fractions()->[f32;4] {
    METER.with(|m| [m.combo as f32 / LEVEL as f32, m.ex as f32 / LEVEL as f32,
                    if m.timeout>0. {m.remaining/m.timeout} else {0.},
                    if m.resonance_duration>0. {m.resonance_frames/m.resonance_duration} else {0.}])
}

// Frontier 857620 weights the struck impact hitzone against the mean of
// damageable body volumes. GU stores impact in byte 1 of each live meat row.
unsafe fn normal_hit_gain(monster:u32, part:u32)->u32 {
    let base=forge::mem::text_addr();
    let body:unsafe extern "C" fn(u32)->*const u8=unsafe{core::mem::transmute(base+0x710b0)};
    let meat:unsafe extern "C" fn(u32,u32)->*const u8=unsafe{core::mem::transmute(base+0xbab60)};
    let mut p=unsafe{body(monster)};
    if p.is_null() || part==0 {return 0;}
    let impact=|zone:u8| {
        let row=unsafe{meat(monster,u32::from(zone))};
        if row.is_null() {0} else {u32::from(unsafe{*row.add(1)})}
    };
    let selected=impact(unsafe{*((part+6) as *const u8)});
    let (mut total,mut count)=(0,0);
    for _ in 0..256 {
        if unsafe{p.cast::<u16>().read_unaligned()}==u16::MAX {break;}
        let flags=unsafe{p.add(10).cast::<u16>().read_unaligned()};
        if flags & 0x100==0 {
            total+=impact(unsafe{*p.add(6)});
            count+=1;
        }
        p=unsafe{p.add(44)};
    }
    if total==0 {return 0;}
    let weight=100*count*selected/total;
    (weight*2).div_ceil(5)
}

pub unsafe fn resolved(actor:u32,hit:u32,monster:u32,part:u32) {
    if actor==0 || monster==0 || !action::melee::BLADE_CONTACT_OFFSETS.contains(&hit.wrapping_sub(actor))
        || !METER.with(|m|m.owner==actor) {return;}
    let (group,slot)=unsafe{action::current(actor)};
    if !action::custom(group,slot) {return;}
    unsafe { crate::release::contact(actor, hit, monster, part); }
    let raw=unsafe{normal_hit_gain(monster,part)};
    if raw==0 {return;}
    METER.with(|m|{
        let gain=if m.short {raw*param(5)/1000} else {raw};
        m.add_combo(gain*param(4)/1000);
        let rate=param(6+(m.combo/LEVEL) as usize);
        m.ex=(m.ex+gain*rate/1000).min(LEVEL*LEVELS);
    });
}

#[forge::hook(offset=0x178030)]
unsafe fn hunter_damage(manager:u32,hit:u32,monster:u32,part:u32,point:u32,actor:u32,
                        factor:*const f32,element:u32)->u32 {
    let result=original!(manager,hit,monster,part,point,actor,factor,element);
    unsafe{resolved(actor,hit,monster,part)};
    result
}
pub fn evading(actor:u32)->bool {
    METER.with(|m| m.owner==actor && m.evade.armed())
}

pub fn reward_evade(actor:u32, slot:u16, power:u16, projectile:bool) {
    let reward = METER.with(|m| {
        if m.owner != actor || !m.evade.connect() { return None; }
        let base = crate::evade::base_gain(power);
        // 13062a0 refreshes the timer at the old level, awards combo,
        // then uses the new level's evade column for EX. Reach only scales hits.
        let points = |rate: u32| (base as f32 * (rate as f32 * 0.001)) as u32;
        m.add_combo(points(crate::evade::combo_rate()));
        m.ex = (m.ex + points(crate::evade::ex_rate(m.combo / LEVEL))).min(LEVEL * LEVELS);
        Some((m.combo, m.ex))
    });
    if let Some((combo, ex)) = reward {
        unsafe { crate::effects::evade(actor); }
        let source = if projectile { "projectile" } else { "contact" };
        forge::log::prelude::info!("Tonfa EX evade avoided monster {} in slot {}, power {}, combo {}, EX {}",source,slot,power,combo,ex);
    }
}

pub fn install(){
    crate::release::install();
    forge::install_hook!(forge::mem::text_addr(),hunter_damage);
    crate::evade::install();
}
