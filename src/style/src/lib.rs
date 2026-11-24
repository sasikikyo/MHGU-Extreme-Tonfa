#![cfg_attr(target_arch = "arm", no_std)]

const TONFA_TYPE: u16 = 25;
const LAST_FAMILY: u16 = 137;
#[cfg(target_arch = "arm")]
mod battle;
#[cfg(target_arch = "arm")]
mod effects;
#[cfg(target_arch = "arm")]
mod evasion_menu;
#[cfg(any(target_arch = "arm", test))]
mod evade;
#[cfg(target_arch = "arm")]
mod gauge;
#[cfg(target_arch = "arm")]
mod guard;
#[cfg(target_arch = "arm")]
mod grip;
#[cfg(target_arch = "arm")]
mod mechanism;
#[cfg(target_arch = "arm")]
mod program;
#[cfg(any(target_arch = "arm", test))]
mod qi;
#[cfg(any(target_arch = "arm", test))]
mod release;

fn equipped(kind: u16, family: u16) -> bool {
    kind & 31 == TONFA_TYPE && (1..=LAST_FAMILY).contains(&family)
}

#[cfg(target_arch = "arm")]
fn game_system() -> u32 {
    unsafe { ((forge::mem::text_addr() + 0x188ca48) as *const u32).read() }
}

#[cfg(target_arch = "arm")]
fn language() -> u8 {
    let system = game_system();
    if system == 0 { 1 } else { unsafe { ((system + 0xa5) as *const u8).read() } }
}

fn hud_color(color: u32, paused: bool) -> u32 {
    if paused { (color & 0xff000000) | ((color & 0x00fcfcfc) >> 2) } else { color }
}

// The Frontier plate was authored over the 1280 x 720 quest HUD.
fn hud_layout(width: f32, height: f32) -> Option<(f32, f32, f32)> {
    if !(width.is_finite() && height.is_finite() && width > 0. && height > 0.) {
        return None;
    }
    let scale = (width / 1280.).min(height / 720.);
    Some(((width - 1280. * scale) * 0.5, (height - 720. * scale) * 0.5, scale))
}

#[cfg(target_arch = "arm")]
mod runtime {
    use core::ffi::CStr;
    use core::sync::atomic::{AtomicBool, Ordering};
    use mhfz_sdk::motion::MotionBanks;
    use mhgu_sdk::style::{self, PASS, Style, StyleRegistry, hunter};

    const STYLE: u32 = 0x5446;
    const KEY: u8 = 16;
    const PATHS: [&CStr; 3] = [c"player\\mot\\frontier_tonfa\\tonfa_0",
                              c"player\\mot\\frontier_tonfa\\tonfa_2",
                              c"player\\mot\\frontier_tonfa\\tonfa_4"];
    static BANKS: MotionBanks = MotionBanks::new([PATHS; 2]);
    static REPORTED_STRIKE: AtomicBool = AtomicBool::new(false);
    // Requests received by the registry are decimal bank/slot IDs.
    const MOTIONS: &[(u32, u16)] = &[(5001,1001), (5002,1002), (5003,1003),
                                      (5101,1401), (5102,1407), (5103,1402), (5113,1410)];

    pub(super) unsafe fn owns(actor: u32) -> bool {
        if actor == 0 || unsafe { hunter::local() } != Some(actor) { return false; }
        let gear = unsafe { ((actor + 5060) as *const u32).read() };
        gear != 0 && super::equipped(unsafe { ((gear + 28) as *const u16).read() },
                                     unsafe { ((gear + 30) as *const u16).read() })
    }

    pub(super) unsafe fn active(actor:u32)->bool {
        (unsafe {owns(actor)}) && style::selected(KEY)==STYLE
    }

    pub(super) unsafe fn source_clip(actor: u32, packed: u16) -> Option<u16> {
        unsafe { BANKS.decode(actor, packed) }
    }

    unsafe extern "C" fn encode(actor: u32, motion: u32) -> u32 {
        if !unsafe { active(actor) } { return PASS; }
        let clip = if motion>>16==1 {motion as u16} else if let Some(clip)=unsafe {crate::guard::recoil_clip(actor,motion)} {clip} else {
            let Some((_,clip))=MOTIONS.iter().find(|(id,_)|*id==motion) else {return PASS};
            *clip
        };
        let Some(packed) = (unsafe { BANKS.encode(actor, clip) }) else { return PASS; };
        static RECOIL_CLIPS:core::sync::atomic::AtomicU32=core::sync::atomic::AtomicU32::new(0);
        if matches!(clip,1450|1451) && motion>>16==0 {
            let bit=1<<(clip-1450);
            if RECOIL_CLIPS.fetch_or(bit,Ordering::Relaxed)&bit==0 {
                forge::log::prelude::info!("Tonfa guard recoil uses Frontier clip {}",clip);
            }
        }
        if clip >= 1400 && !REPORTED_STRIKE.swap(true, Ordering::Relaxed) {
            forge::log::prelude::info!("Extreme Tonfa playing Frontier attack clip {}", clip);
        }
        u32::from(packed)
    }

    unsafe extern "C" fn decode(actor: u32, packed: u32) -> u32 {
        if !unsafe { active(actor) } { return PASS; }
        let Some(clip) = u16::try_from(packed).ok().and_then(|p| unsafe { BANKS.decode(actor,p) })
            else { return PASS; };
        if let Some(motion)=unsafe {crate::guard::recoil_motion(actor,clip)} {return motion;}
        MOTIONS.iter().find(|(_,c)| *c == clip).map_or(32000+u32::from(clip-1000), |(id,_)| *id)
    }

    unsafe extern "C" fn preload(actor: u32) {
        if unsafe { owns(actor) } {
            unsafe { BANKS.preload_for_hunt(actor, 1401); crate::battle::preload(); }
        }
    }

    static EXTREME: Style = Style {
        id: STYLE, weapon: KEY as u32, independent: true, art_slots: 0,
        encode: Some(encode), decode: Some(decode), controller: Some(crate::battle::dispatch),
        request: Some(crate::battle::request), actions: crate::program::ACTIONS, guard: Some(crate::guard::block), commands: Some(crate::battle::bind_commands),
    };

    #[forge::entry]
    fn main() {
        crate::evasion_menu::load();
        style::registry::when_ready(register);
    }

    extern "C" fn register() {
        let Some(registry) = StyleRegistry::current() else { return; };
        let (name, description) = match super::language() {
            8 => (c"穿龙棍·极之型", c"穿龙棍的边境极之型。"),
            7 => (c"穿龍棍·極之型", c"穿龍棍的邊境極之型。"),
            _ => (c"Extreme Tonfa", c"Frontier Extreme Style for Tonfa."),
        };
        match registry.register(&EXTREME, name, description) {
            Ok(()) => {
                crate::guard::bind(STYLE);
                crate::effects::bind(STYLE);
                crate::gauge::bind(STYLE);
                crate::guard::install();
                crate::grip::install();
                crate::battle::install();
                unsafe { style::preload::bind(STYLE, preload); }
                forge::log::prelude::info!("Extreme Tonfa registered on independent weapon key 16");
            }
            Err(e) => forge::log::prelude::warn!("Extreme Tonfa registration failed: {e:?}"),
        }
    }

    #[forge::imgui_free_render]
    fn hud() {
        let Some(actor) = (unsafe { hunter::local() }) else { return; };
        if !unsafe { owns(actor) && hunter::is_quest(actor) } { return; }
        // The hunter stays alive during the result camera. sQuest changes
        // phase before GU removes the actor or returns to town.
        let quest = unsafe { ((forge::mem::text_addr() + 0x188ad90) as *const u32).read() };
        if quest == 0 || unsafe { ((quest + 0x4c) as *const u32).read() } != 1 { return; }
        let list = forge::imgui::DrawList::foreground();
        use forge::sys::imgui::{ImDrawList_GetClipRectMax, ImDrawList_GetClipRectMin, ImVec2};
        let min = unsafe { ImDrawList_GetClipRectMin(list.raw()) };
        let max = unsafe { ImDrawList_GetClipRectMax(list.raw()) };
        let Some((left, top, scale)) = super::hud_layout(max.x - min.x, max.y - min.y) else { return; };
        let point = |x, y| ImVec2 { x: min.x + left + x * scale, y: min.y + top + y * scale };
        // cUIOPauseWindow uses bit 0 of these two masks (60c720).
        let system = super::game_system();
        let paused = system != 0 && unsafe {
            (*((system + 0x7b) as *const u8) | *((system + 0x7c) as *const u8)) & 1 != 0
        };
        for row in include_bytes!("../gauge.rgba-spans").chunks_exact(20) {
            let coord = |i| f32::from_le_bytes(row[i..i+4].try_into().unwrap());
            let col = u32::from_le_bytes(row[16..20].try_into().unwrap());
            list.add_rect_filled(point(coord(0), coord(4)), point(coord(8), coord(12)), super::hud_color(col,paused), 0.);
        }
        let [combo, ex, timer, resonance]=crate::gauge::fractions();
        let strip=|art:&[u8], left:f32, top:f32, width:f32| {
            for row in art.chunks_exact(8) {
                let x=f32::from(row[0]);
                let end=f32::from(row[1]).min(width);
                if x>=end {continue;}
                let y=f32::from(row[2]);
                let col=u32::from_le_bytes(row[4..8].try_into().unwrap());
                list.add_rect_filled(point(109.+(left+x)*0.75,62.+(top+y)*0.75),
                                     point(109.+(left+end)*0.75,62.+(top+y+1.)*0.75),super::hud_color(col,paused),0.);
            }
        };
        for i in 0..5 {
            let ix=i as f32;
            if combo>ix { strip(include_bytes!("../gauge-combo.bin"),35.+24.*ix,29.,28.*(combo-ix).min(1.)); }
            if ex>ix { strip(include_bytes!("../gauge-ex.bin"),41.+28.*ix,5.,28.*(ex-ix).min(1.)); }
        }
        if timer>0. {strip(include_bytes!("../gauge-timer.bin"),8.,8.,28.*timer);}
        if resonance > 0. {
            // The full tracks remain visible beneath the film; its moving edge
            // is the remaining resonance time, independent of combo decay.
            for (x, y, width, height) in [(41.,5.,140.,11.), (35.,29.,124.,12.)] {
                let right = x + width * resonance;
                list.add_rect_filled(point(109.+x*0.75,62.+y*0.75),
                    point(109.+right*0.75,62.+(y+height)*0.75),super::hud_color(0x90ffe8aa,paused),0.);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pause_dims_the_film_and_gauge_without_changing_opacity() {
        assert_eq!(super::hud_color(0x90ffe8aa, false), 0x90ffe8aa);
        assert_eq!(super::hud_color(0x90ffe8aa, true), 0x903f3a2a);
    }
    #[test]
    fn hud_tracks_the_centered_game_view() {
        assert_eq!(super::hud_layout(1280., 720.), Some((0., 0., 1.)));
        assert_eq!(super::hud_layout(2560., 1440.), Some((0., 0., 2.)));
        assert_eq!(super::hud_layout(2560., 1350.), Some((80., 0., 1.875)));
        assert_eq!(super::hud_layout(1280., 800.), Some((0., 40., 1.)));
        assert_eq!(super::hud_layout(0., 720.), None);
    }
    #[test]
    fn all_families_share_the_equipment_gate() {
        for family in 0..=138 {
            for kind in 0..32 {
                assert_eq!(super::equipped(kind | 0x40, family),
                           kind == 25 && (1..=137).contains(&family));
            }
        }
    }
}
