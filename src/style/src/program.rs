use mhfz_sdk::action::{BranchRules, Cursor, Step};
use mhgu_sdk::style::action::Actions;

const DATA: &[u8] = include_bytes!("../actions.bin");
const COUNT: usize = 153;
pub const SPRINT: u8 = 200;
pub const BRAKE: u8 = 201;
pub const BOOST_FALL: u8 = 202;
// Private GU action IDs for Frontier common group 0 actions 107..110.
pub const STEP_FORWARD: u8 = 204;
pub const STEP_BACK: u8 = 205;
pub const STEP_LEFT: u8 = 206;
pub const STEP_RIGHT: u8 = 207;
// Common actions 126..129 repeat each corresponding clip once.
pub const REPEAT_STEP: u8 = 208;
pub const LAST_STEP: u8 = 211;
pub const GUARD_RAISE: u8 = 212;
pub const GUARD_HOLD: u8 = 213;
pub const GUARD_RELEASE: u8 = 214;
const CONTACTS: &[u8] = include_bytes!("../contacts.bin");

const fn word(at: usize) -> usize {
    u32::from_le_bytes([DATA[at], DATA[at+1], DATA[at+2], DATA[at+3]]) as usize
}

pub const ACTIONS: Actions = {
    let mut actions = Actions::new(0);
    let mut index = 0;
    while index < COUNT {
        if word(index * 24) != 0 { actions = actions.with(index as u8); }
        index += 1;
    }
    let mut step=STEP_FORWARD;
    while step<=LAST_STEP {actions=actions.with(step); step+=1;}
    actions.with(SPRINT).with(BRAKE).with(BOOST_FALL).with(GUARD_RAISE).with(GUARD_HOLD).with(GUARD_RELEASE)
};

fn rows(state: u8, field: usize, width: usize) -> &'static [u8] {
    if usize::from(state) >= COUNT { return &[]; }
    let at = usize::from(state) * 24 + field * 8;
    let start = word(at + 4);
    &DATA[start..start+word(at)*width]
}

pub fn step(cursor: &Cursor) -> Option<Step> {
    if cursor.state==BOOST_FALL {
        return Some(Step {op:5,clip:1445,start:0,blend:2,until:0,loops:1});
    }
    // Frontier 9e47b0 and 9ec3f0, normal guard for weapon 11.
    if (GUARD_RAISE..=GUARD_RELEASE).contains(&cursor.state) {
        let clip=1005+u16::from(cursor.state-GUARD_RAISE);
        return (cursor.time.step==0).then_some(Step {
            op:if cursor.state==GUARD_HOLD {5} else {3},clip,start:0,blend:4,until:0,loops:1,
        });
    }
    // Common action table 11894ca0, weapon index 11.
    if matches!(cursor.state, SPRINT | BRAKE) {
        let sprint = cursor.state == SPRINT;
        return (cursor.time.step == 0).then_some(Step {
            op: if sprint { 5 } else { 3 }, clip: if sprint { 1008 } else { 1015 },
            start: 0, blend: 4, until: 0, loops: 1,
        });
    }
    if (STEP_FORWARD..=LAST_STEP).contains(&cursor.state) {
        let clip=[1011,1014,1012,1013][usize::from(cursor.state-STEP_FORWARD)%4];
        return (cursor.time.step==0).then_some(Step {op:3,clip,start:0,blend:4,until:0,loops:1});
    }
    Step::read(rows(cursor.state, 0, 12), cursor.time.step)
}

pub fn cursor(state: u8) -> Cursor {
    static RULES: BranchRules = BranchRules {
        order: &[23, 12, 1, 0, 18, 22, 21, 19, 6, 7, 4, 5, 2, 3, 11, 20],
        combo_override: None,
        art_commands: 0,
    };
    Cursor::new(state, rows(state,1,40), rows(state,2,12), &RULES)
}

// Command 22 arms an EX thrust. Frontier reads its eligibility but never
// commits its dummy destination. Probe with a separate input buffer so the
// hunter can keep holding X across the transition to charge state 87.
pub fn ex_charge_window(cursor: &Cursor) -> bool {
    let mut probe = self::cursor(cursor.state);
    probe.time = cursor.time;
    probe.branch(1 << 22, |condition| condition == 16).is_some()
}

pub struct Contact {
    pub start: u16,
    pub duration: u16,
    pub power: u16,
    pub group: usize,
    pub stun: u8,
}

pub fn contact(index: u16) -> Option<Contact> {
    let row = CONTACTS.chunks_exact(40).nth(usize::from(index))?;
    let half = |i:usize| u16::from_le_bytes([row[i],row[i+1]]);
    Some(Contact {start:half(0),duration:half(2),power:half(4),group:half(16) as usize,stun:row[33]})
}
