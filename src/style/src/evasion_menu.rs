//! Forge setting for the local Extreme Tonfa's skill-adjustable evades.
extern crate alloc;

use core::sync::atomic::{AtomicU8, Ordering::Relaxed};
use forge::{fs::savedata, mt::{fs::{MtFile, OpenMode}, object::Object}};

static LEVEL: AtomicU8 = AtomicU8::new(0);

#[inline(never)]
pub unsafe fn bonus_frames(actor: u32) -> f32 {
    let gear = unsafe { mhfz_sdk::combat::evade_bonus_frames(actor) };
    let floor = match LEVEL.load(Relaxed) { 1 => 6., 2 => 10., _ => 0. };
    gear.max(floor)
}

// Keep this outside the hunter save and Style Registry's own configuration.
fn setting_file(write: Option<u8>) -> Option<u8> {
    if !savedata::mount() { return None; }
    let access = || {
        let path = alloc::format!("{}:/extreme_tonfa_evasion.cfg", savedata::mount_point());
        if write.is_none() && !forge::fs::file_exists(&path) { return Some(0); }
        let path = alloc::ffi::CString::new(path).ok()?;
        let mode = if write.is_some() { OpenMode::Write } else { OpenMode::Read };
        let file = MtFile::open(&path, mode).ok()?;
        let result = if let Some(level) = write {
            (file.write(&[b'0' + level]) == 1).then_some(level)
        } else {
            let mut buf = [0; 1];
            if file.size() == 1 && file.read(&mut buf) == 1 && (b'0'..=b'2').contains(&buf[0]) {
                Some(buf[0] - b'0')
            } else { Some(0) }
        };
        file.close();
        file.destroy();
        result
    };
    let result = access();
    savedata::unmount();
    result
}

pub fn load() {
    let level = setting_file(None).unwrap_or(0);
    LEVEL.store(level, Relaxed);
    forge::log::prelude::info!("Extreme Tonfa evasion setting loaded: {}", level);
}

#[forge::imgui_render]
fn draw() {
    use forge::imgui::{collapsing_header, radio_button};
    let chinese = matches!(crate::language(), 7 | 8);
    let flags = forge::sys::imgui::ImGuiTreeNodeFlags(0);
    if collapsing_header(if chinese {c"穿龙棍招式与操作"} else {c"Extreme Tonfa Moves"}, flags) {
        forge::imgui::text(if chinese {"操作顺序为 Xbox 手柄 / 键盘鼠标。LMB 左键，RMB 右键，MMB 中键。"} else {"Xbox / keyboard and mouse. LMB / RMB / MMB are the mouse buttons."});
        for row in include_str!("../moves.txt").lines() {
            let mut cells = row.trim_start_matches('#').split('|');
            let (Some(zh), Some(en)) = (cells.next(),cells.next()) else {continue};
            let name = if chinese { zh } else { en };
            if let (Some(pad),Some(keys)) = (cells.next(),cells.next()) {
                forge::imgui::text(&alloc::format!("{name}    {pad}    /    {keys}"));
            } else { forge::imgui::text(name); }
        }
    }
    if !collapsing_header(if chinese {c"极之型穿龙棍回避"} else {c"Extreme Style Tonfa Evasion"}, forge::sys::imgui::ImGuiTreeNodeFlags(0)) { return; }
    forge::imgui::text(if chinese {"EX回避需要至少一格上方能量。指定方向，按住防御并轻按收棍键。"} else {"EX Evade requires an upper segment. Aim, hold guard and tap sheathe."});
    forge::imgui::text(if chinese {"喷射回避：防御加回避起跳，再指定方向并按一次防御加回避。"} else {"Aerial boost: guard + dodge to jump, then aim and tap guard + dodge again."});
    let labels = if chinese {[c"默认回避无敌时间", c"回避性能+1 无敌时间", c"回避性能+2 无敌时间"]}
        else {[c"Default Evasion I-frames", c"Evasion+1 I-frames", c"Evasion+2 I-frames"]};
    for (level, label) in labels.into_iter().enumerate() {
        let level = level as u8;
        let selected = LEVEL.load(Relaxed) == level;
        if radio_button(label, selected) && !selected {
            LEVEL.store(level, Relaxed);
            if setting_file(Some(level)).is_none() {
                forge::log::prelude::warn!("Could not save Extreme Tonfa evasion setting");
            }
        }
    }
}
