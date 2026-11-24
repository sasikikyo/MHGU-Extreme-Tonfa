//! Execute Frontier action steps on GU's game thread.
use crate::program;
use crate::program::{SPRINT, BRAKE, STEP_FORWARD, STEP_BACK, STEP_LEFT, STEP_RIGHT, REPEAT_STEP, LAST_STEP};
use crate::program::{BOOST_FALL, GUARD_RAISE, GUARD_HOLD, GUARD_RELEASE};
use mhfz_sdk::sprint::{self, Flick};
use mhgu_sdk::style::action::command::CommandSet;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use mhfz_sdk::{action::Cursor, combat::HitWindow, sound::Clock};
use mhgu_sdk::{combat::collision::CachedHitShape, style::{
    ActionRequest, Input, PASS, NATIVE_ACTION, action::{self, Attack, MotionFrame, stun},
    movement, sound::Bank,
}};

static BANKS: [Bank;2] = [
    Bank::new(c"sound\\se\\pl\\divine_tonfa\\class"),
    Bank::new(c"sound\\se\\pl\\divine_tonfa\\weapon"),
];
macro_rules! shapes {
    ($($path:literal),+) => { [$(CachedHitShape::new($path)),+] };
}
static LONG: [CachedHitShape;11] = shapes!(
    c"player\\hit\\frontier_tonfa\\long1",c"player\\hit\\frontier_tonfa\\long2",
    c"player\\hit\\frontier_tonfa\\long3",c"player\\hit\\frontier_tonfa\\long4",
    c"player\\hit\\frontier_tonfa\\long5",c"player\\hit\\frontier_tonfa\\long6",
    c"player\\hit\\frontier_tonfa\\long7",c"player\\hit\\frontier_tonfa\\long8",
    c"player\\hit\\frontier_tonfa\\long9",c"player\\hit\\frontier_tonfa\\long10",
    c"player\\hit\\frontier_tonfa\\long11");
static SHORT: [CachedHitShape;11] = shapes!(
    c"player\\hit\\frontier_tonfa\\short1",c"player\\hit\\frontier_tonfa\\short2",
    c"player\\hit\\frontier_tonfa\\short3",c"player\\hit\\frontier_tonfa\\short4",
    c"player\\hit\\frontier_tonfa\\short5",c"player\\hit\\frontier_tonfa\\short6",
    c"player\\hit\\frontier_tonfa\\short7",c"player\\hit\\frontier_tonfa\\short8",
    c"player\\hit\\frontier_tonfa\\short9",c"player\\hit\\frontier_tonfa\\short10",
    c"player\\hit\\frontier_tonfa\\short11");

macro_rules! gu {
    ($addr:expr, $($arg:expr),+) => {{
        let f:unsafe extern "C" fn($(gu!(@ty $arg)),+) -> u32 =
            core::mem::transmute((forge::mem::text_addr()+$addr) as usize);
        f($($arg),+)
    }};
    (@ty $arg:expr) => { u32 }
}

pub(crate) unsafe fn preload() {
    unsafe { crate::effects::preload(); }
    for b in &BANKS { unsafe { b.preload() }; }
    for shape in LONG.iter().chain(&SHORT) { unsafe { shape.preload() }; }
}

pub(crate) unsafe fn release_sound(actor: u32, cue: u8, position: [f32; 3]) {
    unsafe { BANKS[0].play_at(actor, cue, position); }
}

#[derive(Default)]
struct Battle {
    flick: Flick,
    step_followup: Option<u8>,
    guard_age: f32,
    recoiling: bool,
    actor: u32,
    cursor: Option<Cursor>,
    packed: u16,
    short: bool,
    ex_charge: bool,
    flurry_meter_left: bool,
    flurry_punches: usize,
    poise: bool,
    airborne: bool,
    air_knockback: bool,
    // A second press during takeoff is consumed at the Frontier cancel frame.
    launch_evade: Option<(bool, usize)>,
    air_stalls:u8,
    launch_height:f32,
    flight_phase:u8,
    audio: Clock,
}

impl Battle {
    fn guard_chord(actor:u32, button:u16)->bool {
        let held=unsafe { ((actor+5108) as *const u16).read() };
        let pressed=unsafe { ((actor+5112) as *const u16).read() };
        let guard=if unsafe { mhgu_sdk::style::input::auxiliary_shoulder_mask() }==0x800 {0x40} else {0x800};
        let chord=guard|button;
        held & chord==chord && pressed & chord!=0
    }
    fn ground_step(&mut self)->bool {
        self.begin([STEP_FORWARD,STEP_BACK,STEP_LEFT,STEP_RIGHT][self.direction()])
    }
    fn ground_attack(&self, shoulder:bool, chord:bool, x:bool, a:bool)->Option<u8> {
        if shoulder && chord && crate::gauge::can_spend() {Some(57)}
        else if shoulder && x {Some(39)} else if shoulder && a {Some(35)}
        else if chord {Some(5)} else if x {Some(14)} else if a {Some(19)} else {None}
    }
    fn direction(&self)->usize {
        Self::actor_direction(self.actor)
    }
    fn actor_direction(actor:u32)->usize {
        match unsafe{movement::relative_stick_yaw(actor)}.unwrap_or(0) {
            0x6000..=0xa000=>1,
            0x2000..0x6000=>3,
            0xa001..=0xe000=>2,
            _=>0,
        }
    }

    fn ex_followup(&self, first:bool)->u8 {
        // Frontier 1309050 splits diagonals from cardinals on the second
        // half of the two lateral EX evasions (targets 160 and 161).
        let yaw=unsafe{movement::relative_stick_yaw(self.actor)}.map(|v|v as i16);
        let (left,right)=match yaw {
            None=>(6,7),
            Some(-0x705b..=-0x4fa5)=>(111,105),
            Some(-0x305b..=-0x0fa5)=>(106,100),
            Some(0x0fa5..=0x305b)=>(108,102),
            Some(0x4fa5..=0x705b)=>(109,103),
            Some(i16::MIN..=-0x6001 | 0x6001..=i16::MAX)=>(110,104),
            Some(-0x6000..=-0x4001)=>(6,105),
            Some(-0x4000..=-0x2001)=>(6,106),
            Some(0x2001..=0x4000)=>(108,7),
            Some(0x4001..=0x6000)=>(109,7),
            _=>(107,101),
        };
        if first {left} else {right}
    }

    fn velocity(&self, forward:f32, up:f32, gravity:f32) {
        // Source vectors are per 30 Hz tick in local space. GU velocities
        // are world-space per 60 Hz frame, accelerations per frame squared.
        unsafe {
            let yaw=f32::from(((self.actor+4076) as *const u16).read())*core::f32::consts::TAU/65536.;
            let (x,z)=libm::sincosf(yaw);
            ((self.actor+4112) as *mut [f32;3]).write([x*forward/2.,up/2.,z*forward/2.]);
            ((self.actor+4128) as *mut [f32;3]).write([0.,gravity/4.,0.]);
        }
    }

    fn flight(&mut self,id:u8,clip:u16,frame:f32)->Option<u8>{
        if !self.airborne {return None;}
        let hit=AIR_CONTACT.swap(false,Ordering::Relaxed);
        if id==56 && hit {return Some(34);}
        if matches!(clip,1022|1023) && frame<52. {return None;}
        if id==34 && self.flight_phase==0 {
            if frame<20. {return None;}
            self.velocity(0.,8.*1.0909091,-1.0909091);
            self.flight_phase=1;
        }
        let vy=(self.actor+4116) as *mut f32;
        let ay=(self.actor+4132) as *mut f32;
        unsafe {
            if matches!(id,52..=54) && hit {
                vy.write(4.*1.0909091/2.);
                ay.write(-1.0909091/4.);
            }
            if matches!(id,55|56) {
                let (descent,terminal)=if id==55 {(-4.3636365,-261.81818)} else {(-2.1818182,-130.90909)};
                if self.flight_phase==0 && vy.read()<=0. {
                    self.flight_phase=1; ay.write(descent/4.);
                } else if self.flight_phase==1 && vy.read()<=terminal/2. {
                    self.flight_phase=2; ay.write(0.);
                }
            }
            if ((self.actor+68) as *const f32).read()-self.launch_height>=500. && vy.read()>0. {
                vy.write(0.);
            }
            gu!(0x539f4c,self.actor);
            if vy.read()<0. && action::land(self.actor) {
                return Some(match id {55=>32,56=>38,_=>37});
            }
        }
        None
    }
    fn jump(&self)->u8 {
        let relative=unsafe{movement::relative_stick_yaw(self.actor)}.unwrap_or(0);
        if (0x4001..0xc000).contains(&relative) {12} else {11}
    }

    fn sound(&mut self, clip:u16, frame:f32, first:Option<f32>) {
        let timeline: &[u8] = if self.short { include_bytes!("../sound-short.bin") }
                              else { include_bytes!("../sound-long.bin") };
        unsafe { self.audio.update(self.actor,&BANKS,timeline,Some(clip),frame,first) };
    }

    fn hit(&mut self, id:u16, late:f32) {
        let Some(row)=program::contact(id) else { return };
        if !(1..=11).contains(&row.group) {return;}
        let index=row.group-1;
        let shapes=if self.short { &SHORT } else { &LONG };
        let Some(shape)=(unsafe { shapes[index].load() }) else {
            if !LOGGED_SHAPE.swap(true,Ordering::Relaxed) {forge::log::prelude::warn!("Tonfa shape {} unavailable",row.group);}
            return
        };
        let sample=unsafe { MotionFrame::sample(self.actor) };
        let Some(window)=HitWindow::new(row.start,row.duration,sample.frame,late) else { return };
        // The projected thrust overlaps the physical fist by six frames.
        // Give it its own contact while its shape stays on the same arm.
        let secondary=matches!(row.group,2|4|6|10|11);
        let flags=Attack::WEAPON_MODIFIERS|Attack::NO_MOUNT|if secondary {Attack::SECONDARY} else {0};
        // Frontier counts each flurry shell when it starts (11e4c40).
        // A missed punch still advances the sequence. Both arms share it.
        let flurry=(79..=90).contains(&id);
        const PUNCHES: &[[u8; 2]] = include_bytes!("../flurry-power.bin").as_chunks::<2>().0;
        const _: () = assert!(include_bytes!("../flurry-power.bin").len() == 2 * PUNCHES.len()
                              && !PUNCHES.is_empty());
        let index=self.flurry_punches.min(PUNCHES.len()-1);
        let [motion, percent]=PUNCHES[index];
        let power=crate::gauge::motion_power(self.actor, if flurry {u16::from(motion)} else {row.power});
        let attack=Attack {template:if secondary {2} else {1},power,
            delay:window.delay,duration:window.duration,flags,shape:shape.as_raw(),no_sharpness_cost:0};
        let stun_value=if self.short {row.stun} else {row.stun.saturating_add(2)};
        let accepted=unsafe { stun::submit(self.actor,&attack,stun_value) };
        if accepted {
            // The class-100 shell snapshots the EX evade multiplier on creation.
            let qi = crate::gauge::motion_power(self.actor, crate::qi::shell_gain(id));
            crate::release::submitted(usize::from(secondary), id, qi);
        }
        if flurry && accepted {
            unsafe { crate::effects::flurry_punch(self.actor,self.flurry_punches.saturating_add(1)); }
            // GU 2b784c writes the equipped attribute flags and amount.
            // 11e61f4 scales the finished Frontier contact and truncates.
            let hand=usize::from(secondary);
            let hit=self.actor+action::melee::BLADE_CONTACT_OFFSETS[hand];
            let attribute=(hit+0x50) as *mut u16;
            let original=unsafe { attribute.read() };
            let multiplier=f32::from(percent)*0.01;
            let scaled=(f64::from(original)*f64::from(multiplier)) as u16;
            unsafe { attribute.write(scaled); }
            if self.flurry_punches==0 || (self.flurry_punches==index && index>0 && PUNCHES[index-1]!=PUNCHES[index]) {
                let flags=unsafe { ((hit+0x48) as *const u32).read() };
                forge::log::prelude::info!("Tonfa flurry punch {} uses motion value {}, attribute {} -> {} flags {}",self.flurry_punches+1,power,original,scaled,flags);
            }
            self.flurry_punches=self.flurry_punches.saturating_add(1);
        }
        if !LOGGED_HIT.swap(true,Ordering::Relaxed) {
            forge::log::prelude::info!("Tonfa contact {} at frame {}, power {}, KO {}, accepted {}",id,sample.frame,power,stun_value,accepted);
        }
    }

    fn events(&mut self) {
        while let Some((kind,value,late))=self.cursor.as_mut().and_then(Cursor::next_event) {
            match kind {
                1 => unsafe {gu!(0x282fd8,self.actor,u32::from(value&2),u32::from(value&1),0);},
                2 => unsafe {gu!(0x2a2560,self.actor,u32::from(value!=0));},
                3 => self.hit(value,late),
                11 => {
                    self.poise=false;
                    unsafe{((self.actor+10288) as *mut f32).write((2.*f32::from(value)-late).max(0.));}
                }
                12 => self.poise=true,
                13|14 => unsafe {
                    let extra=if kind==14 {crate::evasion_menu::bonus_frames(self.actor)} else {0.};
                    let timer=(self.actor+11072) as *mut f32;
                    let frames=(2.*f32::from(value)+extra-late).max(0.);
                    timer.write(timer.read().max(frames));
                    if let Some(cursor)=self.cursor.as_ref() { crate::gauge::evade_window(cursor.state,frames); }
                },
                17 => unsafe{movement::stamina(self.actor,-150.,false);},
                18 => crate::gauge::spend(),
                19 if value==0 || value==2 => {
                    self.short=value==0;
                    crate::gauge::reach(self.short);
                }
                _ => (),
            }
        }
    }

    fn finish(&mut self) {
        if self.cursor.as_ref().is_some_and(|c| c.state==87) {unsafe {crate::effects::stop_charge();}}
        self.cursor=None;
        self.airborne=false;
        self.air_knockback=false;
        self.ex_charge=false;
        unsafe { gu!(0x2b6e5c,self.actor); }
    }

    fn play(&mut self) { self.play_from(None); }
    fn play_from(&mut self, restart:Option<f32>) {
        let Some(cursor)=self.cursor.as_mut() else {return};
        let Some(step)=program::step(cursor) else { self.finish(); return };
        let start=restart.unwrap_or(f32::from(step.start));
        let blend=if restart.is_some(){0.}else{f32::from(step.blend as i16).max(0.)/2.};
        let packed=unsafe { action::play_motion(self.actor,0x10000|u32::from(step.clip),start,blend) };
        let Some(packed)=packed else { self.finish(); return };
        self.packed=packed;
        cursor.time.frame=start;
        cursor.time.cycles=0;
        cursor.time.blend=blend;
        self.sound(step.clip,start,Some(start));
        unsafe { crate::effects::motion(self.actor, step.clip, start-1., start); }
        self.events();
    }

    fn begin(&mut self,id:u8)->bool {
        if !program::ACTIONS.contains(id) || !unsafe { action::accept_custom_start(self.actor,id) } {return false;}
        if self.cursor.as_ref().is_some_and(|c| c.state==87) {unsafe {crate::effects::stop_charge();}}
        let actor=self.actor;
        crate::release::begin(actor,id);
        crate::gauge::evade_window(id,0.);
        self.poise=false;
        self.step_followup=None;
        self.launch_evade=None;
        if id==GUARD_RAISE {self.guard_age=0.;}
        let air_move=matches!(id,9|11|12|23|25|26|34|40..=43|52..=56|114..=117|143..=152|BOOST_FALL);
        self.airborne=air_move;
        self.air_knockback=false;
        self.flight_phase=0;
        if id!=87 {self.ex_charge=false;}
        if !matches!(id,125|126) {
            self.flurry_punches=0;
        }
        if matches!(id,57|137|138) {
            self.flurry_meter_left=crate::gauge::flurry_cost();
        }
        AIR_CONTACT.store(false,Ordering::Relaxed);
        unsafe {
            ((actor+5) as *mut u8).write(1);
            for off in action::melee::BLADE_CONTACT_OFFSETS { ((actor+off+176) as *mut u32).write(0); }
            if matches!(id,GUARD_RAISE|GUARD_HOLD) {
                // Same action flags GU sets when it raises a shield.
                gu!(0x282fd8,actor,0x8000,0,0);
                gu!(0x283238,actor,0,0x10,0);
            } else {
                gu!(0x282fd8,actor,if air_move {2} else {0},u32::from(id!=SPRINT),0);
            }
            if id==SPRINT {
                movement::steer(actor);
                gu!(0x283238,actor,0,0x180,0);
                gu!(0x2e9b7c,actor,2000,0);
                gu!(0x283238,actor,0,0x1000,0);
            }
            if (STEP_FORWARD..=LAST_STEP).contains(&id) {
                // Tonfa override 9ba320 invokes a41950 for seven 30 Hz
                // ticks of evasion, then pays the normal ground evade cost.
                movement::stamina(actor,-150.,false);
                let timer=(actor+11072) as *mut f32;
                timer.write(timer.read().max(14.+crate::evasion_menu::bonus_frames(actor)));
                mhfz_sdk::combat::bladescale_evade(actor,mhfz_sdk::combat::DrawnEvade::Roll);
            }
            if gu!(0x2a2444,actor)==0 { gu!(0x2a264c,actor,1); }
            else { gu!(0x2a2560,actor,1); }
            if matches!(id,11|12|23) {
                // 1318A70 leaves height to w11goku until clip frame 52,
                // then adds forward momentum and gravity each logic frame.
                let forward=match id {11=>7.5, 23=>17.5, _=>-5.};
                self.velocity(forward,0.,-1.0909091);
                self.launch_height=((actor+68) as *const f32).read();
                self.air_stalls=3;
            }
            if id==55 {self.velocity(7.,40.,-10.);}
            if id==56 {self.velocity(15.,20.,-2.5);}
            if id==26 {
                // Frontier 131e72c initializes vertical velocity and acceleration.
                self.velocity(0.,6.545455,-1.0909091);
                self.air_stalls=3;
            }
            if matches!(id,40..=43 | 114..=117 | 145..=148) {
                self.velocity(0.,((actor+4116) as *const f32).read(),-0.36363637);
                self.air_stalls=3;
            }
            if matches!(id,149..=152) {
                self.velocity(0.,0.,-0.36363637);
                self.air_stalls=3;
            }
            if id==BOOST_FALL {
                // The directional clip supplies the boost's root travel once.
                // Carry the vertical velocity through to ground contact.
                self.velocity(0.,2.*((actor+4116) as *const f32).read(),-0.36363637);
            }
            if matches!(id,52..=54) && self.air_stalls>0 {
                self.air_stalls-=1;
                ((actor+4112) as *mut [f32;3]).write([0.;3]);
                ((actor+4128) as *mut [f32;3]).write([0.,-0.72727275/4.,0.]);
            }
            if air_move {
                let flags=(actor+10057) as *mut u8;
                flags.write(flags.read()|0x40);
            }
        }
        self.cursor=Some(program::cursor(id));
        if STARTS.fetch_add(1,Ordering::Relaxed)<18 {
            forge::log::prelude::info!("Tonfa action {} started",id);
        }
        self.play();
        if id==87 {unsafe {crate::effects::charge(actor,0);}}
        true
    }

    fn tick(&mut self, input:&Input)->bool {
        let actor=self.actor;
        let current=unsafe {action::current(actor)};
        let chord=input.weapon_command(11);
        let x=input.weapon_command(0);
        let a=input.weapon_command(1)||input.weapon_command(2);
        let shoulder=input.command(27);
        // GU's filtered button words remain available in the air even when
        // the common command table disables sheathing and ordinary rolling.
        let ex_pressed=input.auxiliary_pressed!=0 || Self::guard_chord(actor,0x10);
        let boost_pressed=Self::guard_chord(actor,0x80);
        let was_recoiling=self.recoiling;
        self.recoiling=matches!(current,(32,3..=5));
        if (x || a || chord) && INPUTS.fetch_add(1,Ordering::Relaxed)<12 {
            let held=unsafe{((actor+5108) as *const u16).read()};
            forge::log::prelude::info!("Tonfa input {:?} group {}, slot {}, held {:#x}",input.weapon,current.0,current.1,held);
        }
        let custom=action::custom_id(current.0,current.1);
        if was_recoiling && (movement::ground_idle(current) || custom==Some(GUARD_RAISE)) {
            self.guard_age=38.;
            return self.begin(if shoulder {GUARD_HOLD} else {GUARD_RELEASE});
        }
        let elapsed=unsafe {((actor+28) as *const f32).read()};
        let drawn=unsafe {gu!(0x2a2444,actor)}!=0;
        let stamina=unsafe {((actor+5088) as *const f32).read()};
        let flick=if drawn && !self.airborne {
            self.flick.update(input,elapsed) && stamina>0.
        } else {self.flick=Flick::default(); false};
        if custom.is_none() {
            if self.cursor.as_ref().is_some_and(|c| c.state==87) {unsafe {crate::effects::stop_charge();}}
            // Only a native hit reaction interrupting Tonfa flight opens this window.
            // Recheck the current floor so a late press cannot cancel the get-up.
            if matches!(current,(0x40,4..=36)) && (self.airborne || self.air_knockback) {
                let position=unsafe { ((actor+64) as *const [f32;3]).read() };
                self.air_knockback=unsafe { mhgu_sdk::style::particle::ground_heights(actor,position) }
                    .is_some_and(|(floor,surface)|position[1]>surface.unwrap_or(floor)+1.);
                let dodge=unsafe { ((actor+5112) as *const u16).read() } & 0x80!=0;
                let alive=unsafe { ((actor+0x13d6) as *const i16).read() }>0;
                if self.air_knockback && dodge && drawn && alive && stamina>=150. && self.begin(26) {
                    unsafe { movement::stamina(actor,-150.,false); }
                    forge::log::prelude::info!("Tonfa aerial breakfall from damage slot {}",current.1);
                    return true;
                }
            } else {
                self.air_knockback=false;
            }
            self.cursor=None;
            self.airborne=false;
            self.ex_charge=false;
            // From a standing start GU can accept the common draw before
            // polling the weapon controller, just as it does for a dodge.
            if current==(1,1) && x && unsafe{((actor+5) as *const u8).read()}==0 {
                return self.begin(0);
            }
            if !drawn {
                return x && (movement::ground_idle(current) || movement::sheathed_movement(current))
                    && unsafe {action::enter_draw_attack(actor,0)} && self.begin(0);
            }
            // The native recoil controller returns to shared guard. Resume
            // Tonfa playback after GU has applied its collision response.
            if matches!(current,(32,0|1|2)) {
                self.guard_age=38.;
                return self.begin(if shoulder {GUARD_HOLD} else {GUARD_RELEASE});
            }
            // GU accepts a shared dodge before the style's next controller
            // tick. Its phase-zero entry is the same R+B edge from neutral.
            if current==(1,6) && unsafe{((actor+5) as *const u8).read()}==0 {
                if shoulder {return self.begin(self.jump());}
                if stamina>=150. {return self.ground_step();}
            }
            if !movement::ground_idle(current) && current!=(1,1) {return false;}
            let next=if ex_pressed && crate::gauge::can_spend() {[77,80,84,85][self.direction()]}
                else if shoulder && input.command(3) {self.jump()}
                else if input.command(3) && stamina>=150. {return self.ground_step();}
                else if let Some(next)=self.ground_attack(shoulder,chord,x,a) {next}
                else if flick {SPRINT} else if shoulder {GUARD_RAISE} else {return false};
            return self.begin(next);
        }
        let id=custom.unwrap();
        if unsafe {((actor+5) as *const u8).read()}==0 {return self.begin(id);}
        if self.cursor.as_ref().is_none_or(|c| c.state!=id)
            || unsafe {((actor+1204) as *const u16).read()}!=self.packed {
            self.finish();return true;
        }
        let sample=unsafe { MotionFrame::sample(actor) };
        let cursor=self.cursor.as_mut().unwrap();
        let previous_frame=cursor.time.frame;
        let wrapped=cursor.time.observe(sample.frame,sample.blend);
        let Some(step)=program::step(cursor) else {self.finish();return true};
        if self.poise {unsafe{let timer=(actor+10288) as *mut f32;timer.write(timer.read().max(4.));}}
        self.sound(step.clip,sample.frame,None);
        unsafe { crate::effects::motion(actor, step.clip, if wrapped {-1.} else {previous_frame}, sample.frame); }
        self.events();
        let cycles=self.cursor.as_ref().unwrap().time.cycles;
        if id==87 && cycles==0 && !wrapped {
            for (at,stage) in [(30.,1),(64.,2)] {
                if previous_frame<at && sample.frame>=at {unsafe {crate::effects::charge(actor,stage);}}
            }
        }
        if matches!(id,GUARD_RAISE|GUARD_HOLD) {
            self.guard_age+=elapsed;
            if ex_pressed && crate::gauge::can_spend() {
                return self.begin([77,80,84,85][self.direction()]);
            }
            if shoulder {
                if input.command(3) && stamina>=150. {return self.begin(self.jump());}
                if let Some(next)=self.ground_attack(true,chord,x,a) {return self.begin(next);}
                if flick {return self.begin(SPRINT);}
            } else if self.guard_age>36. {return self.begin(GUARD_RELEASE);}
            if id==GUARD_RAISE && (sample.complete() || wrapped) {return self.begin(GUARD_HOLD);}
            if id==GUARD_HOLD && sample.ended && !wrapped {self.play();}
            return true;
        }
        if (STEP_FORWARD..=LAST_STEP).contains(&id) {
            // 130e660 buffers the latest edge and commits it at 30.
            // The backward step accepts from 10 and commits at 40.
            let (open,execute)=if (id-STEP_FORWARD)%4==1 {(10.,40.)} else {(0.,30.)};
            if sample.frame>=open {
                let next=if ex_pressed && crate::gauge::can_spend() {Some([77,80,84,85][self.direction()])}
                    else if shoulder && chord && crate::gauge::can_spend() {Some(57)}
                    else if shoulder && input.command(3) && stamina>=150. {Some(self.jump())}
                    else if shoulder && x {Some(39)} else if shoulder && a {Some(35)}
                    else if chord {Some(5)} else if x {Some(140)} else if a {Some(49)}
                    else if flick {Some(SPRINT)} else {None};
                if next.is_some() {self.step_followup=next;}
            }
            if sample.frame>=execute && let Some(next)=self.step_followup {return self.begin(next);}
            if sample.frame>=execute && id<REPEAT_STEP && input.command(3) && stamina>=150. {
                return self.begin(REPEAT_STEP+self.direction() as u8);
            }
        }
        if (id==BRAKE || (id==SPRINT && (cycles>0 || sample.frame>=24.)))
            && ex_pressed && crate::gauge::can_spend() {
            return self.begin([77,80,84,85][self.direction()]);
        }
        if id==SPRINT || (id==BRAKE && sample.frame>=42.) {
            if id==SPRINT && elapsed>0. { unsafe {movement::drain_running_stamina(actor)}; }
            let moving=id==SPRINT && unsafe {movement::steer(actor)};
            // The common sprint handler polls ordinary actions only after
            // 9a1060 opens the weapon's input window (Tonfa frame 30).
            if id==SPRINT && !sprint::can_brake(sample.frame,cycles) {return true;}
            if id==SPRINT && (!moving || stamina<=0.) {return self.begin(BRAKE);}
            if input.command(3) && stamina>=150. {
                if shoulder {return self.begin(if id==SPRINT {23} else {self.jump()});}
                return self.ground_step();
            }
            if id==BRAKE {
                if let Some(next)=self.ground_attack(shoulder,chord,x,a) {return self.begin(next);}
            } else if !chord && (x || a) {
                return self.begin(if a {1} else if self.short {2} else {3});
            }
            if input.command(13) && unsafe {action::transition_to(actor,1,2)} {self.cursor=None; return true;}
            if id==SPRINT {
                if sample.complete() && !wrapped {
                    self.play_from(Some(70.));
                    if let Some(cursor)=self.cursor.as_mut() {cursor.time.cycles=cycles.saturating_add(1);}
                }
                return true;
            }
            if flick {return self.begin(SPRINT);}
        }
        if let Some(next)=self.flight(id,step.clip,sample.frame) {return self.begin(next);}
        if matches!(id,145..=148) {
            if wrapped || sample.complete() {return self.begin(BOOST_FALL);}
            return true;
        }
        if id==BOOST_FALL {return true;}
        if matches!(id,125|126) {
            // Frame keys from the ZZ loop handler at 1322290. A hitstop
            // repeat consumes nothing, and a skipped frame still pays.
            for key in [70.,86.,108.,130.,148.,170.,192.,214.,232.,254.,274.,296.] {
                let crossed=if wrapped {key>previous_frame || key<=sample.frame}
                            else {key>previous_frame && key<=sample.frame};
                if crossed {
                    self.flurry_meter_left=crate::gauge::flurry_cost();
                }
            }
        }
        if matches!(id,57|125|126|137|138) && !self.flurry_meter_left {return self.begin(58);}
        // The source command classifier emits 12 whenever the authored
        // automatic branch window is open, including charge progression.
        let mut commands=1<<12;
        if flick {commands|=1<<11;}
        if matches!(id,11|12|23) {
            if ex_pressed && crate::gauge::can_spend() {
                self.launch_evade=Some((true,self.direction()));
            } else if boost_pressed {
                self.launch_evade=Some((false,self.direction()));
            }
        }
        if ex_pressed || matches!(self.launch_evade,Some((true,_))) {commands|=1<<18;}
        if self.airborne && (boost_pressed || matches!(self.launch_evade,Some((false,_)))) {commands|=2;}
        if shoulder && chord {commands|=(1<<19)|(1<<5);}
        else if shoulder && x {commands|=1<<6;} else if shoulder && a {commands|=1<<7;}
        else if chord {commands|=1<<5;} else if x {commands|=1<<2;} else if a {commands|=1<<3;}
        if input.command(3) { commands|=if shoulder {2} else {1}; }
        let held=unsafe{((actor+5108) as *const u16).read()};
        if matches!(id,87|91..=93|140) && !self.ex_charge && shoulder && crate::gauge::can_spend()
            && program::ex_charge_window(self.cursor.as_ref().unwrap()) {
            crate::gauge::spend();
            self.ex_charge=true;
        }
        if self.ex_charge {commands|=1<<23;}
        // Frontier checks the primary attack button for a thrust release.
        if held & 0x20==0 {commands|=if self.ex_charge {1<<21} else {1<<4};}
        let short=self.short;
        let ex_charge=self.ex_charge;
        let cursor=self.cursor.as_mut().unwrap();
        if id==87 && sample.complete() && !wrapped && cycles==0 {
            self.play_from(Some(44.));
            if let Some(cursor)=self.cursor.as_mut(){cursor.time.cycles=1;}
            return true;
        }
        let stamina=unsafe{((actor+5088) as *const f32).read()}>=150.;
        let target=cursor.branch(commands,|cond| match cond {
            0|7|21..=23=>true,
            1|24=>stamina,
            2|17=>short,
            3|18=>!short,
            14=>ex_charge,
            15|16|20=>crate::gauge::can_spend(),
            _=>false,
        });
        if let Some(next)=target.filter(|&t|t<153) && self.begin(next as u8) {return true;}
        if target==Some(153) && stamina {return self.ground_step();}
        if target==Some(154) {return self.begin(self.jump());}
        if target==Some(155) {return self.begin([40,43,41,42][self.direction()]);}
        if target==Some(156) {return self.begin([114,117,115,116][self.direction()]);}
        if target==Some(157) {
            let dir=self.launch_evade.map_or_else(||self.direction(),|(_,dir)|dir);
            return self.begin([145,148,146,147][dir]);
        }
        if target==Some(158) {return self.begin(SPRINT);}
        if target==Some(159) {return self.begin([77,80,84,85][self.direction()]);}
        if matches!(target,Some(160|161)) {return self.begin(self.ex_followup(target==Some(160)));}
        if target==Some(162) {
            let dir=self.launch_evade.map_or_else(||self.direction(),|(_,dir)|dir);
            return self.begin([149,152,150,151][dir]);
        }
        let cursor=self.cursor.as_mut().unwrap();
        if step.complete(cursor.time,sample,wrapped) {
            cursor.time.step+=1;
            self.play();
        }
        true
    }
}

struct Local(UnsafeCell<Option<Battle>>);
static SPRINT_TERRAIN:CommandSet=CommandSet::new(&[]).retaining_native(1,movement::CLIMBS);
pub(crate) unsafe extern "C" fn bind_commands(actor:u32,table:u32)->*const CommandSet {
    let (group,slot)=unsafe {action::current(actor)};
    if table==2000 && action::custom_id(group,slot)==Some(SPRINT) {&SPRINT_TERRAIN}
    else {core::ptr::null()}
}
// Registry controller callbacks are synchronous on the game thread.
unsafe impl Sync for Local {}
static LOCAL:Local=Local(UnsafeCell::new(None));
static BUSY:AtomicBool=AtomicBool::new(false);
static LOGGED_HIT:AtomicBool=AtomicBool::new(false);
static OWNER:AtomicU32=AtomicU32::new(0);
static RESOLVED:AtomicU32=AtomicU32::new(0);
static AIR_CONTACT:AtomicBool=AtomicBool::new(false);
#[forge::hook(offset=0x169d44)]
unsafe fn monster_contact(hit:u32,target:u32,kind:u32)->u32 {
    let actor=OWNER.load(Ordering::Relaxed);
    if actor!=0 && kind==1 && target!=0 &&
        action::melee::BLADE_CONTACT_OFFSETS.contains(&hit.wrapping_sub(actor)) {
        let (group,slot)=unsafe{action::current(actor)};
        if let Some(id)=action::custom_id(group,slot) {
            if matches!(id,52..=54|56) {AIR_CONTACT.store(true,Ordering::Relaxed);}
            if RESOLVED.fetch_add(1,Ordering::Relaxed)<12 {
                forge::log::prelude::info!("Frontier Tonfa action {} connected with a monster",id);
            }
        }
    }
    original!(hit,target,kind)
}
pub(crate) fn install() {
    forge::install_hook!(forge::mem::text_addr(),monster_contact);
    crate::gauge::install();
}
static LOGGED_SHAPE:AtomicBool=AtomicBool::new(false);
static STARTS:core::sync::atomic::AtomicU32=core::sync::atomic::AtomicU32::new(0);
static INPUTS:core::sync::atomic::AtomicU32=core::sync::atomic::AtomicU32::new(0);
static REQUESTS:core::sync::atomic::AtomicU32=core::sync::atomic::AtomicU32::new(0);

pub(crate) unsafe extern "C" fn dispatch(actor:u32,input:*const Input)->u32 {
    if BUSY.swap(true,Ordering::Acquire) {return PASS;}
    let local=unsafe { &mut *LOCAL.0.get() };
    let handled=if let Some(input)=unsafe {input.as_ref()} {
        OWNER.store(actor,Ordering::Relaxed);
        let elapsed=unsafe{((actor+28) as *const f32).read()};
        crate::gauge::tick(actor,elapsed);
        crate::release::tick(actor,elapsed);
        if local.as_ref().is_none_or(|s|s.actor!=actor) {*local=Some(Battle{actor,..Battle::default()});}
        local.as_mut().unwrap().tick(input)
    } else {unsafe {crate::effects::stop_charge();} OWNER.store(0,Ordering::Relaxed);crate::gauge::tick(0,0.);crate::release::tick(0,0.);*local=None;false};
    BUSY.store(false,Ordering::Release);
    if handled {0} else {PASS}
}

pub(crate) unsafe extern "C" fn request(actor:u32,request:*const ActionRequest)->u32 {
    let Some(req)=(unsafe {request.as_ref()}) else {return PASS};
    if REQUESTS.fetch_add(1,Ordering::Relaxed)<24 {
        forge::log::prelude::info!("Tonfa request group {}, slot {}, draw {}",req.group,req.slot,req.draw_route);
    }
    if req.draw_route && matches!(req.slot,7..=9) {return 0;}
    if req.draw_route {return PASS;}
    let (group,slot)=unsafe {action::current(actor)};
    if action::custom(group,slot) {return PASS;}
    let buttons=unsafe {((actor+5156) as *const u32).read()};
    let common=unsafe {((actor+5140) as *const u32).read()};
    let shoulder=common & (1<<27)!=0;
    if req.slot==13 && movement::ground_idle((group,slot)) && Battle::guard_chord(actor,0x10)
        && crate::gauge::can_spend() {
        return [77,80,84,85][Battle::actor_direction(actor)];
    }
    match req.slot {
        0..=3 | 7..=14 if shoulder && buttons&(1<<11)!=0 && crate::gauge::can_spend() => 57,
        0..=3 | 7..=14 if shoulder && buttons&1!=0 => 39,
        0..=3 | 7..=14 if shoulder && buttons&6!=0 => 35,
        0..=3 | 7..=14 if buttons & (1<<11)!=0 => 5,
        0..=3 | 7..=14 if buttons & 6!=0 => 19,
        0..=3 | 7..=14 if buttons&1!=0 => 14,
        13 if shoulder && movement::ground_idle((group,slot)) => u32::from(GUARD_RAISE),
        0..=3 | 7..=14 => PASS,
        _ => NATIVE_ACTION,
    }
}
