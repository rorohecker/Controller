use std::{
    env, fs,
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use aftertouch_protocol::TriggerSetting;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Archive {
    schema_version: u8,
    active_id: String,
    #[serde(default = "default_true")]
    auto_apply: bool,
    profiles: Vec<Profile>,
    #[serde(default)]
    trigger_presets: Vec<TriggerPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tactile_clips: Option<Vec<crate::audio_haptics::authoring::EffectClip>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    id: String,
    name: String,
    description: String,
    tag: String,
    #[serde(default)]
    favorite: bool,
    #[serde(default, alias = "match_exe")]
    match_exe: String,
    #[serde(default)]
    match_title: String,
    #[serde(default)]
    match_path: String,
    #[serde(default = "default_true")]
    auto_apply: bool,
    triggers: TriggerPair,
    haptics: Haptics,
    #[serde(default)]
    gyro_aim: GyroAimSettings,
    #[serde(default)]
    controls: crate::input_map::controls::ControlProfile,
    #[serde(default)]
    remap: RemapSettings,
    #[serde(default)]
    input_effects: InputEffectSettings,
    #[serde(default)]
    actions_enabled: bool,
    #[serde(default)]
    actions: Vec<crate::game_actions::ActionRule>,
    #[serde(default)]
    game_link: crate::game_link::LinkSettings,
    #[serde(default)]
    weapon_bindings: Vec<WeaponBinding>,
    #[serde(default)]
    game_shortcuts: Vec<GameShortcut>,
    #[serde(default)]
    session: SessionSettings,
    #[serde(default)]
    community: CommunityMetadata,
    /// Optional lightbar / player-LED state applied with the profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lightbar: Option<LightbarSettings>,
    /// Optional reference (id) to a saved audio preset; resolved by the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    audio: Option<String>,
    /// Optional controller volumes (headphone/speaker/microphone) applied with the profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    audio_preset: Option<AudioPreset>,
}

/// Lightbar state saved with a profile. The UI writes `color` as `#rrggbb`; an `[r,g,b]` array is
/// accepted on read and normalised to the hex form. Unknown keys are ignored so newer UIs can add fields.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LightbarSettings {
    #[serde(deserialize_with = "deserialize_color")]
    color: String,
    /// Player indicator LED bitmask, bits 0..=4.
    player_leds: u8,
    /// "static", "off", "breathe", "pulse", "rainbow", "battery" or "lowBatteryBlink".
    mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    period_ms: Option<u32>,
}
fn deserialize_color<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Color {
        Hex(String),
        Rgb([u8; 3]),
    }
    Ok(match Color::deserialize(d)? {
        Color::Hex(h) => h,
        Color::Rgb([r, g, b]) => format!("#{r:02x}{g:02x}{b:02x}"),
    })
}
impl Default for LightbarSettings {
    fn default() -> Self {
        Self {
            color: "#005aff".into(),
            player_leds: 0,
            mode: "static".into(),
            period_ms: None,
        }
    }
}
impl LightbarSettings {
    fn validate(&self) -> bool {
        let hex = self.color.len() == 7
            && self.color.starts_with('#')
            && self.color[1..].bytes().all(|b| b.is_ascii_hexdigit());
        hex && self.player_leds <= 0b11111
            && matches!(
                self.mode.as_str(),
                "static" | "off" | "breathe" | "pulse" | "rainbow" | "battery" | "lowBatteryBlink"
            )
            && self.period_ms.is_none_or(|p| (100..=20_000).contains(&p))
    }
}

/// Controller audio volumes saved with a profile (same ranges as `SetAudioVolumes`).
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AudioPreset {
    #[serde(skip_serializing_if = "Option::is_none")]
    headphone: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    speaker: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    microphone: Option<u8>,
}
impl AudioPreset {
    fn validate(&self) -> bool {
        self.headphone.is_none_or(|v| v <= 127)
            && self.speaker.is_none_or(|v| v <= 100)
            && self.microphone.is_none_or(|v| v <= 127)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct SessionSettings {
    enabled: bool,
    input_route: String,
    audio: String,
    audio_gain: u8,
    hide_hint: bool,
}
impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            input_route: "keep".into(),
            audio: "keep".into(),
            audio_gain: 50,
            hide_hint: false,
        }
    }
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct CommunityMetadata {
    author: String,
    version: String,
    game_version: String,
    source_url: String,
    license: String,
    updated_at: String,
    reports: Vec<CompatibilityReport>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompatibilityReport {
    platform: String,
    transport: String,
    result: String,
    date: String,
    game_version: String,
    notes: String,
}
impl CommunityMetadata {
    fn validate(&self) -> bool {
        [
            &self.author,
            &self.version,
            &self.game_version,
            &self.license,
        ]
        .iter()
        .all(|s| s.chars().count() <= 80 && !s.contains(char::is_control))
            && self.updated_at.len() <= 32
            && self.source_url.len() <= 1024
            && (self.source_url.is_empty()
                || (self.source_url.starts_with("https://")
                    && !self.source_url.contains(char::is_whitespace)
                    && !self.source_url.contains(char::is_control)))
            && self.reports.len() <= 20
            && self.reports.iter().all(|r| {
                matches!(r.platform.as_str(), "windows" | "macos" | "linux")
                    && matches!(r.transport.as_str(), "usb" | "bluetooth")
                    && matches!(r.result.as_str(), "works" | "partial" | "broken")
                    && r.date.len() <= 32
                    && r.game_version.chars().count() <= 80
                    && r.notes.chars().count() <= 300
                    && !r.notes.contains('\0')
            })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameShortcut {
    button: u8,
    modifier: Option<u8>,
    action: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TriggerPreset {
    id: String,
    name: String,
    triggers: TriggerPair,
    input_effects: InputEffectSettings,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeaponBinding {
    weapon: String,
    preset_id: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
struct TriggerPair {
    left: TriggerSetting,
    right: TriggerSetting,
}

#[derive(Debug, Serialize, Deserialize)]
struct Haptics {
    shape: String,
    frequency: u16,
    attack: u16,
    release: u16,
    strength: u8,
    pan: i16,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GyroAimSettings {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_sens")]
    sensitivity: f32,
    #[serde(default = "default_axis")]
    yaw_scale: f32,
    #[serde(default = "default_axis")]
    pitch_scale: f32,
    #[serde(default)]
    invert_y: bool,
    #[serde(default = "default_activation")]
    activation: String,
    #[serde(default)]
    bias: [i16; 3],
    #[serde(default = "default_gyro_deadzone")]
    deadzone: u16,
    #[serde(default = "default_gyro_smoothing")]
    smoothing: f32,
    /// "hold", "disable" or "toggle" (what the aim button does).
    #[serde(default = "default_gyro_gate")]
    gate: String,
    #[serde(default)]
    stick_off_threshold: f32,
    #[serde(default)]
    smooth_threshold: f32,
    #[serde(default)]
    acceleration: f32,
    #[serde(default)]
    accel_threshold: f32,
    #[serde(default)]
    flick_without_gyro: bool,
}
fn default_gyro_gate() -> String {
    "hold".into()
}

fn default_sens() -> f32 {
    1.0
}
fn default_axis() -> f32 {
    1.0
}
fn default_activation() -> String {
    "always".into()
}
fn default_gyro_deadzone() -> u16 {
    90
}
fn default_gyro_smoothing() -> f32 {
    0.18
}

impl Default for GyroAimSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            sensitivity: 1.0,
            yaw_scale: 1.0,
            pitch_scale: 1.0,
            invert_y: false,
            activation: "always".into(),
            bias: [0; 3],
            deadzone: default_gyro_deadzone(),
            smoothing: default_gyro_smoothing(),
            gate: default_gyro_gate(),
            stick_off_threshold: 0.0,
            smooth_threshold: 0.0,
            acceleration: 0.0,
            accel_threshold: 0.0,
            flick_without_gyro: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RemapSettings {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    bindings: Vec<RemapBinding>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RemapBinding {
    button: u8,
    vk: u16,
    #[serde(default)]
    modifier: Option<u8>,
    #[serde(default = "default_hold")]
    behavior: String,
    #[serde(default)]
    sequence: Vec<u16>,
    #[serde(default)]
    chord: Vec<u16>,
    /// Macro-program button behaviour (PC mode); omitted when the default.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "programMode"
    )]
    program_mode: Option<crate::input_map::ProgramMode>,
    /// Layer modifiers, long-press / double-tap, turbo and consume (camelCase, all optional).
    #[serde(flatten)]
    extras: crate::input_map::RemapExtras,
}

fn default_hold() -> String {
    "hold".into()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InputEffectSettings {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_click_threshold")]
    threshold: f32,
    #[serde(default = "default_click_strength")]
    strength: u8,
    #[serde(default = "default_weapon_kind")]
    kind: String,
    #[serde(default = "default_weapon_rate")]
    rate: u8,
    #[serde(default)]
    spacing_ms: u16,
    #[serde(default)]
    kick_ms: u16,
    #[serde(default = "default_kick_style")]
    kick_style: String,
    #[serde(flatten)]
    tuning: aftertouch_protocol::BurstTuning,
}
fn default_click_threshold() -> f32 {
    0.82
}
fn default_click_strength() -> u8 {
    55
}
fn default_weapon_kind() -> String {
    "click".into()
}
fn default_weapon_rate() -> u8 {
    10
}
fn default_kick_style() -> String {
    "return".into()
}
impl Default for InputEffectSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: default_click_threshold(),
            strength: default_click_strength(),
            kind: default_weapon_kind(),
            rate: default_weapon_rate(),
            spacing_ms: 0,
            kick_ms: 0,
            kick_style: default_kick_style(),
            tuning: aftertouch_protocol::BurstTuning::default(),
        }
    }
}

impl InputEffectSettings {
    fn validate(&self) -> Result<(), String> {
        if !(0.2..=0.95).contains(&self.threshold)
            || self.strength > 100
            || !(3..=20).contains(&self.rate)
            || self.tuning.validate(self.kick_style == "return").is_err()
            || !matches!(self.kick_style.as_str(), "return" | "strike" | "stop")
            || (self.spacing_ms != 0 && !(50..=1000).contains(&self.spacing_ms))
            || (self.kick_ms != 0 && !(20..=250).contains(&self.kick_ms))
            || (self.kind == "burst"
                && self.kick_style != "strike"
                && self.kick_ms != 0
                && u32::from(self.kick_ms) + 20
                    > if self.spacing_ms == 0 {
                        1000 / u32::from(self.rate.max(1))
                    } else {
                        u32::from(self.spacing_ms)
                    })
            || !matches!(
                self.kind.as_str(),
                "click" | "pistol" | "sniper" | "burst" | "smg"
            )
        {
            return Err("Input-driven trigger settings are out of range".into());
        }
        Ok(())
    }
}

fn paths() -> Result<(PathBuf, PathBuf), String> {
    let base = config_base()?;
    Ok((
        base.join("CONTROLLER").join("profiles.json"),
        base.join("Aftertouch").join("profiles.json"),
    ))
}

pub(crate) fn config_base() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or("APPDATA is unavailable".into())
    }
    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library/Application Support"))
            .ok_or("HOME is unavailable".into())
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(xdg) = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            return Ok(xdg);
        }
        env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".config"))
            .ok_or("HOME is unavailable".into())
    }
}

fn validate(data: Value) -> Result<Archive, String> {
    let mut archive: Archive = serde_json::from_value(data).map_err(|e| e.to_string())?;
    // Older archives gain explicit remap layer and action defaults.
    if matches!(archive.schema_version, 1 | 2) {
        archive.schema_version = 3;
    }
    if archive.schema_version != 3 || archive.profiles.is_empty() || archive.profiles.len() > 50 {
        return Err("Unsupported profile schema or profile count".into());
    }
    if !archive.profiles.iter().any(|p| p.id == archive.active_id) {
        return Err("Active profile does not exist".into());
    }
    if archive.trigger_presets.len() > 100 {
        return Err("Up to 100 trigger presets can be saved".into());
    }
    crate::audio_haptics::authoring::validate_library(
        archive.tactile_clips.as_deref().unwrap_or(&[]),
    )
    .map_err(str::to_string)?;
    let mut preset_ids = std::collections::HashSet::new();
    for preset in &archive.trigger_presets {
        if preset.id.is_empty()
            || preset.id.len() > 80
            || !preset_ids.insert(&preset.id)
            || preset.name.trim().is_empty()
            || preset.name.chars().count() > 40
        {
            return Err("Invalid trigger preset name or ID".into());
        }
        preset.triggers.left.validate().map_err(str::to_string)?;
        preset.triggers.right.validate().map_err(str::to_string)?;
        preset.input_effects.validate()?;
    }
    let mut ids = std::collections::HashSet::new();
    for p in &archive.profiles {
        p.controls.validate().map_err(str::to_string)?;
        if !matches!(p.session.input_route.as_str(), "keep" | "native" | "xinput")
            || !matches!(p.session.audio.as_str(), "keep" | "off" | "system" | "game")
            || p.session.audio_gain > 100
            || !p.community.validate()
        {
            return Err("Invalid session settings or profile source details".into());
        }
        let mut shortcuts = std::collections::HashSet::new();
        if p.game_shortcuts.len() > 12
            || p.game_shortcuts.iter().any(|s| {
                s.button >= 23
                    || matches!(s.button, 6 | 7 | 16 | 18)
                    || s.modifier
                        .is_some_and(|m| m >= 23 || m == s.button || matches!(m, 6 | 7 | 16 | 18))
                    || !shortcuts.insert((s.button, s.modifier))
                    || !matches!(
                        s.action.as_str(),
                        "nextProfile" | "previousProfile" | "nextWeapon" | "previousWeapon"
                    )
            })
        {
            return Err("Invalid game shortcut".into());
        }
        if !ids.insert(&p.id)
            || p.id.is_empty()
            || p.id.len() > 80
            || p.name.trim().is_empty()
            || p.name.chars().count() > 40
            || p.description.chars().count() > 180
            || p.tag.chars().count() > 40
            || p.match_exe.len() > 80
            || p.match_exe.contains(['/', '\\'])
            || p.match_title.chars().count() > 120
            || p.match_title.contains('\0')
            || p.match_path.chars().count() > 1024
            || p.match_path.contains('\0')
            || (p.match_exe.is_empty() && (!p.match_title.is_empty() || !p.match_path.is_empty()))
        {
            return Err("Profile name, ID, description, or tag is invalid".into());
        }
        if p.lightbar.as_ref().is_some_and(|l| !l.validate())
            || p.audio_preset.as_ref().is_some_and(|a| !a.validate())
            || p.audio.as_ref().is_some_and(|a| {
                a.is_empty() || a.chars().count() > 80 || a.contains(char::is_control)
            })
        {
            return Err("Profile lightbar or audio preset reference is invalid".into());
        }
        let mut actions = crate::game_actions::ActionMapper::default();
        actions.set(p.actions.clone()).map_err(str::to_string)?;
        p.game_link.validate().map_err(str::to_string)?;
        let mut weapons = std::collections::HashSet::new();
        if p.weapon_bindings.len() > 100
            || p.weapon_bindings.iter().any(|b| {
                b.weapon.trim().is_empty()
                    || b.weapon.len() > 80
                    || !weapons.insert(b.weapon.to_lowercase())
                    || !preset_ids.contains(&b.preset_id)
            })
        {
            return Err(
                "Weapon mappings need distinct names and an existing trigger preset".into(),
            );
        }
        p.triggers.left.validate().map_err(str::to_string)?;
        p.triggers.right.validate().map_err(str::to_string)?;
        let h = &p.haptics;
        if !crate::audio_haptics::VoiceClip::is_known_shape(&h.shape)
            || !(40..=300).contains(&h.frequency)
            || !(2..=200).contains(&h.attack)
            || !(25..=650).contains(&h.release)
            || h.strength > 100
            || !(-100..=100).contains(&h.pan)
        {
            return Err("Haptic settings are out of range".into());
        }
        let g = &p.gyro_aim;
        if !(0.05..=8.0).contains(&g.sensitivity)
            || !(0.0..=4.0).contains(&g.yaw_scale)
            || !(0.0..=4.0).contains(&g.pitch_scale)
            || !matches!(
                g.activation.as_str(),
                "always" | "l1" | "r1" | "l2" | "r2" | "touchpad"
            )
            || g.bias.iter().any(|n| i32::from(*n).abs() > 4096)
            || g.deadzone > 512
            || !g.smoothing.is_finite()
            || !(0.0..=0.85).contains(&g.smoothing)
            || !matches!(g.gate.as_str(), "hold" | "disable" | "toggle")
            || !(0.0..=1.0).contains(&g.stick_off_threshold)
            || !(0.0..=1000.0).contains(&g.smooth_threshold)
            || !(0.0..=2.0).contains(&g.acceleration)
            || !(0.0..=1000.0).contains(&g.accel_threshold)
        {
            return Err("Gyro aim settings are out of range".into());
        }
        p.input_effects.validate()?;
        let mut mapped_buttons = std::collections::HashSet::new();
        if p.remap.bindings.len() > 46
            || p.remap.bindings.iter().any(|b| {
                b.button >= 23
                    || b.vk == 0
                    || b.vk > 255
                    || b.modifier
                        .is_some_and(|modifier| modifier >= 23 || modifier == b.button)
                    || !matches!(b.behavior.as_str(), "hold" | "tap" | "macro")
                    || (b.behavior == "macro" && (b.sequence.is_empty() || b.sequence.len() > 5))
                    || (b.behavior != "macro" && !b.sequence.is_empty())
                    || b.chord.len() > 8
                    || b.chord.iter().any(|&vk| vk == 0 || vk > 255)
                    || (b.behavior == "macro" && !b.chord.is_empty())
                    || b.sequence.iter().any(|&vk| vk == 0 || vk > 255)
                    || b.extras.modifiers.len() > 3
                    || b.extras.modifiers.iter().any(|&m| m >= 23 || m == b.button)
                    || b.extras.long_press_ms > 2000
                    || b.extras.double_tap_ms > 1000
                    || b.extras.turbo_ms > 500
                    || (b.extras.long_press_ms > 0 && b.extras.double_tap_ms > 0)
                    || !mapped_buttons.insert({
                        let mut mods = b.extras.modifiers.clone();
                        mods.sort_unstable();
                        (
                            b.button,
                            b.modifier,
                            mods,
                            b.extras.long_press_ms,
                            b.extras.double_tap_ms,
                        )
                    })
            })
        {
            return Err("Remap bindings are invalid".into());
        }
    }
    Ok(archive)
}

pub fn load() -> Result<Option<Value>, String> {
    let (current, legacy) = paths()?;
    load_from_paths(&current, &legacy)
}

fn load_from_paths(
    current: &std::path::Path,
    legacy: &std::path::Path,
) -> Result<Option<Value>, String> {
    let source = if current.exists() {
        current
    } else if legacy.exists() {
        legacy
    } else {
        return Ok(None);
    };
    let bytes = fs::read(source).map_err(|e| e.to_string())?;
    if bytes.len() > 1_000_000 {
        return Err("Profile archive is too large".into());
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let archive = validate(data)?;
    let result = serde_json::to_value(archive).map_err(|e| e.to_string())?;
    if source == legacy {
        // Keep the original archive intact. A failed copy must not hide usable profiles.
        if let Some(folder) = current.parent()
            && fs::create_dir_all(folder).is_ok()
        {
            let _ = write_archive(
                current,
                &serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(Some(result))
}

pub fn save(data: Value) -> Result<(), String> {
    let archive = validate(data)?;
    let json = serde_json::to_vec_pretty(&archive).map_err(|e| e.to_string())?;
    if json.len() > 1_000_000 {
        return Err("Profile archive is too large".into());
    }
    let (file, _) = paths()?;
    fs::create_dir_all(file.parent().ok_or("Profile folder is unavailable")?)
        .map_err(|e| e.to_string())?;
    save_at(&file, &json)
}

fn save_at(file: &std::path::Path, json: &[u8]) -> Result<(), String> {
    if fs::read(file).ok().as_deref() == Some(json) {
        return Ok(());
    }
    snapshot(file, false)?;
    write_archive(file, json)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    id: String,
    created_ms: u64,
    profiles: usize,
}

fn backup_stamp(id: &str) -> Option<u128> {
    let digits = id.strip_prefix("profiles-")?.strip_suffix(".json")?;
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Backup files are write-once, so a validation result per path never goes stale.
type BackupCache = std::sync::Mutex<std::collections::HashMap<PathBuf, Option<usize>>>;
fn validated_backup_cache() -> &'static BackupCache {
    static CACHE: std::sync::OnceLock<BackupCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Backup ids in the folder, newest first, without reading or validating contents.
fn backup_names(file: &std::path::Path) -> Vec<(String, u128)> {
    let folder = file.with_file_name("profile-backups");
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut names: Vec<(String, u128)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            backup_stamp(&id).map(|stamp| (id, stamp))
        })
        .collect();
    names.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
    names
}

fn history(file: &std::path::Path) -> Result<Vec<Backup>, String> {
    let folder = file.with_file_name("profile-backups");
    let mut backups = Vec::new();
    for (id, stamp) in backup_names(file) {
        let path = folder.join(&id);
        let cached = validated_backup_cache()
            .lock()
            .ok()
            .and_then(|c| c.get(&path).copied());
        let profiles = match cached {
            Some(result) => result,
            None => {
                let result = fs::read(&path)
                    .ok()
                    .filter(|bytes| bytes.len() <= 1_000_000)
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                    .and_then(|data| validate(data).ok())
                    .map(|archive| archive.profiles.len());
                if let Ok(mut cache) = validated_backup_cache().lock() {
                    if cache.len() > 256 {
                        cache.clear();
                    }
                    cache.insert(path, result);
                }
                result
            }
        };
        if let Some(profiles) = profiles {
            backups.push(Backup {
                id,
                created_ms: (stamp / 1_000_000).min(u128::from(u64::MAX)) as u64,
                profiles,
            });
        }
    }
    Ok(backups)
}

fn snapshot(file: &std::path::Path, force: bool) -> Result<(), String> {
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    // Keep an unreadable archive intact rather than replacing it with defaults.
    if bytes.len() > 1_000_000 {
        return Err(
            "Existing profile archive is too large. Export or repair it before saving.".into(),
        );
    }
    const REPAIR: &str = "Saving is paused so the damaged file is not overwritten. Restore a backup (recover_profiles / restore_profile_backup) to continue.";
    validate(
        serde_json::from_slice(&bytes)
            .map_err(|e| format!("Existing profiles need repair: {e}. {REPAIR}"))?,
    )
    .map_err(|e| format!("Existing profiles need repair: {e}. {REPAIR}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    // Throttle on file names alone; no backup is read for the common automatic case.
    let names = backup_names(file);
    if !force
        && names
            .first()
            .is_some_and(|(_, s)| stamp / 1_000_000 < s / 1_000_000 + 60_000)
    {
        return Ok(());
    }
    let folder = file.with_file_name("profile-backups");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    write_archive(&folder.join(format!("profiles-{stamp}.json")), &bytes)?;
    // The new snapshot plus the 19 newest older ones are kept.
    for (old, _) in names.iter().skip(19) {
        fs::remove_file(folder.join(old)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn backups(create: bool) -> Result<Vec<Backup>, String> {
    let (file, _) = paths()?;
    if create {
        snapshot(&file, true)?;
    }
    history(&file)
}

fn restore_at(file: &std::path::Path, id: &str) -> Result<Value, String> {
    backup_stamp(id).ok_or("Invalid backup name")?;
    let bytes =
        fs::read(file.with_file_name("profile-backups").join(id)).map_err(|e| e.to_string())?;
    if bytes.len() > 1_000_000 {
        return Err("Backup is too large".into());
    }
    let data = serde_json::to_value(validate(
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
    )?)
    .map_err(|e| e.to_string())?;
    // A corrupt current file remains available for manual recovery.
    if fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .is_some_and(|v| validate(v).is_ok())
    {
        snapshot(file, true)?;
    } else if let Ok(current) = fs::read(file) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        write_archive(
            &file.with_file_name(format!("profiles-corrupt-{stamp}.json")),
            &current,
        )?;
    }
    write_archive(
        file,
        &serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?,
    )?;
    Ok(data)
}

pub fn restore(id: &str) -> Result<Value, String> {
    restore_at(&paths()?.0, id)
}

/// Describes whether the current archive loads and, if not, what can recover it.
pub fn recovery_status() -> Result<Value, String> {
    let (current, legacy) = paths()?;
    recovery_status_at(&current, &legacy)
}

fn recovery_status_at(
    current: &std::path::Path,
    legacy: &std::path::Path,
) -> Result<Value, String> {
    match load_from_paths(current, legacy) {
        Ok(_) => Ok(serde_json::json!({"ok":true,"recoverable":false,"savesBlocked":false})),
        Err(error) => {
            let newest = history(current)?.into_iter().next();
            Ok(serde_json::json!({
                "ok": false,
                "message": error,
                "recoverable": newest.is_some(),
                "savesBlocked": true,
                "backup": newest,
            }))
        }
    }
}

/// Restores the newest valid backup over a corrupt archive (the corrupt file is kept aside).
pub fn recover() -> Result<Value, String> {
    let (file, legacy) = paths()?;
    if load_from_paths(&file, &legacy).is_ok() {
        return Err("The current profiles load fine; nothing to recover.".into());
    }
    let newest = history(&file)?
        .into_iter()
        .next()
        .ok_or("No valid profile backup exists to recover from")?;
    restore_at(&file, &newest.id)
}

/// Validates a profile archive (also used to vet imported single-profile archives).
pub fn validate_archive(data: Value) -> Result<Value, String> {
    serde_json::to_value(validate(data)?).map_err(|e| e.to_string())
}

#[cfg(windows)]
pub(crate) fn downloads_folder() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath};
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(pv: *const std::ffi::c_void);
    }
    let mut raw: *mut u16 = std::ptr::null_mut();
    // SAFETY: SHGetKnownFolderPath fills `raw` with a CoTaskMem string that is freed below.
    unsafe {
        let hr = SHGetKnownFolderPath(&FOLDERID_Downloads, 0, std::ptr::null_mut(), &mut raw);
        if hr < 0 || raw.is_null() {
            if !raw.is_null() {
                CoTaskMemFree(raw as *const _);
            }
            return None;
        }
        let mut len = 0;
        while *raw.add(len) != 0 {
            len += 1;
        }
        let path = std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw as *const _);
        Some(PathBuf::from(path))
    }
}
#[cfg(not(windows))]
pub(crate) fn downloads_folder() -> Option<PathBuf> {
    let home = PathBuf::from(env::var_os("HOME")?);
    // XDG user dirs may relocate Downloads.
    if let Ok(text) = fs::read_to_string(home.join(".config/user-dirs.dirs"))
        && let Some(line) = text
            .lines()
            .find(|l| l.trim_start().starts_with("XDG_DOWNLOAD_DIR="))
    {
        let value = line.split_once('=')?.1.trim().trim_matches('"');
        let path = PathBuf::from(value.replace("$HOME", &home.to_string_lossy()));
        if path.is_absolute() {
            return Some(path);
        }
    }
    Some(home.join("Downloads"))
}

pub(crate) fn write_archive(file: &std::path::Path, json: &[u8]) -> Result<(), String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temporary = file.with_extension(format!("{}.{}.tmp", std::process::id(), stamp));
    let result = (|| -> Result<(), String> {
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        output.write_all(json).map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        fs::rename(&temporary, file).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn export(data: Value) -> Result<String, String> {
    let archive = validate(data)?;
    let json = serde_json::to_vec_pretty(&archive).map_err(|e| e.to_string())?;
    let folder = downloads_folder()
        .or_else(|| {
            env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|user| PathBuf::from(user).join("Downloads"))
        })
        .ok_or("Downloads folder is unavailable")?;
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    for suffix in 0..100 {
        let name = if suffix == 0 {
            format!("CONTROLLER-profiles-{stamp}.json")
        } else {
            format!("CONTROLLER-profiles-{stamp}-{suffix}.json")
        };
        let path = folder.join(name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(&json).map_err(|e| e.to_string())?;
                return Ok(path.to_string_lossy().into_owned());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Could not find a free export filename".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tactile_library_and_action_snapshots_survive_native_archive_validation() {
        let mut data = sample_profile();
        let clip = serde_json::json!({"id":"tap","name":"Tap","author":"Local","license":"CC0-1.0","tags":["soft"],"cues":[{"lane":"leftGrip","kind":"pulse","startMs":0,"durationMs":100,"strength":35.0}]});
        data["tactileClips"] = serde_json::json!([clip.clone()]);
        data["profiles"][0]["actions"] = serde_json::json!([{"id":"roll","name":"Roll","source":"press","button":1,"durationMs":120,"timeline":clip}]);
        let encoded = serde_json::to_value(validate(data.clone()).unwrap()).unwrap();
        assert_eq!(encoded["tactileClips"][0]["license"], "CC0-1.0");
        assert_eq!(
            encoded["profiles"][0]["actions"][0]["timeline"]["name"],
            "Tap"
        );
        data["tactileClips"] = serde_json::json!([]);
        assert!(validate(data.clone()).is_ok()); // action snapshots remain independent
        data["profiles"][0]["actions"][0]["haptic"] =
            serde_json::json!({"shape":"sine","frequency":90,"strength":40});
        assert!(validate(data).is_err());
        let legacy = serde_json::to_value(validate(sample_profile()).unwrap()).unwrap();
        assert!(legacy.get("tactileClips").is_none());
    }
    #[test]
    fn gyro_and_binding_extras_roundtrip_and_old_profiles_default() {
        let old = validate(sample_profile()).unwrap();
        let g = &old.profiles[0].gyro_aim;
        assert_eq!(
            (g.gate.as_str(), g.acceleration, g.flick_without_gyro),
            ("hold", 0.0, false)
        );
        let mut data = sample_profile();
        data["profiles"][0]["gyroAim"] = serde_json::json!({"enabled":true,"sensitivity":1,"yawScale":1,"pitchScale":1,"invertY":false,"activation":"l1","gate":"toggle","stickOffThreshold":0.4,"smoothThreshold":80,"acceleration":1.5,"accelThreshold":120,"flickWithoutGyro":true});
        data["profiles"][0]["remap"] = serde_json::json!({"enabled":true,"bindings":[
            {"button":1,"vk":65,"behavior":"tap","modifier":4,"modifiers":[5],"layerOnly":true,"toggleLayer":true,"longPressMs":400,"turboMs":0,"consume":true},
            {"button":1,"vk":66,"behavior":"hold","modifier":4,"modifiers":[5],"doubleTapMs":250,"turboMs":50},
            {"button":2,"vk":0x44,"behavior":"hold","programMode":"whileHeld"}]});
        let saved = serde_json::to_value(validate(data).unwrap()).unwrap();
        let p = &saved["profiles"][0];
        assert_eq!(p["gyroAim"]["gate"], "toggle");
        assert_eq!(
            p["gyroAim"]["stickOffThreshold"].as_f64().unwrap().round() as i32,
            0
        );
        assert_eq!(p["gyroAim"]["accelThreshold"], 120.0);
        assert_eq!(p["gyroAim"]["flickWithoutGyro"], true);
        let b = &p["remap"]["bindings"];
        assert_eq!(b[0]["modifiers"], serde_json::json!([5]));
        assert_eq!(b[0]["layerOnly"], true);
        assert_eq!(b[0]["toggleLayer"], true);
        assert_eq!(b[0]["longPressMs"], 400);
        assert_eq!(b[0]["consume"], true);
        assert_eq!(b[1]["doubleTapMs"], 250);
        assert_eq!(b[1]["turboMs"], 50);
        assert_eq!(b[2]["programMode"], "whileHeld");
        assert!(validate(saved.clone()).is_ok());
        for (path, v) in [
            ("gate", serde_json::json!("sideways")),
            ("stickOffThreshold", serde_json::json!(1.5)),
            ("acceleration", serde_json::json!(3)),
            ("smoothThreshold", serde_json::json!(5000)),
        ] {
            let mut bad = saved.clone();
            bad["profiles"][0]["gyroAim"][path] = v;
            assert!(validate(bad).is_err(), "{path}");
        }
        for (k, v) in [
            ("longPressMs", 2001),
            ("doubleTapMs", 1001),
            ("turboMs", 501),
        ] {
            let mut bad = saved.clone();
            bad["profiles"][0]["remap"]["bindings"][2][k] = v.into();
            assert!(validate(bad).is_err(), "{k}");
        }
    }
    #[test]
    fn ui_lightbar_and_audio_preset_roundtrip_through_save() {
        let mut data = sample_profile();
        data["profiles"][0]["lightbar"] = serde_json::json!({"color":"#12ab9f","playerLeds":21,"mode":"lowBatteryBlink","periodMs":2000});
        data["profiles"][0]["audioPreset"] =
            serde_json::json!({"headphone":100,"speaker":60,"microphone":127});
        data["profiles"][0]["audio"] = "bridge-preset-1".into();
        let saved = serde_json::to_value(validate(data).unwrap()).unwrap();
        let p = &saved["profiles"][0];
        assert_eq!(p["lightbar"]["color"], "#12ab9f");
        assert_eq!(p["lightbar"]["playerLeds"], 21);
        assert_eq!(p["lightbar"]["mode"], "lowBatteryBlink");
        assert_eq!(p["lightbar"]["periodMs"], 2000);
        assert_eq!(p["audioPreset"]["speaker"], 60);
        assert_eq!(p["audio"], "bridge-preset-1");
        assert!(validate(saved.clone()).is_ok());
        // Legacy array colours are normalised; partial presets and unknown lightbar keys are fine.
        let mut legacy = sample_profile();
        legacy["profiles"][0]["lightbar"] =
            serde_json::json!({"color":[255,0,16],"playerLeds":0,"mode":"static","future":1});
        legacy["profiles"][0]["audioPreset"] = serde_json::json!({"headphone":5});
        let out = serde_json::to_value(validate(legacy).unwrap()).unwrap();
        assert_eq!(out["profiles"][0]["lightbar"]["color"], "#ff0010");
        assert!(out["profiles"][0]["audioPreset"].get("speaker").is_none());
        for bad in [
            serde_json::json!({"lightbar":{"color":"red","playerLeds":0,"mode":"static"}}),
            serde_json::json!({"lightbar":{"color":"#000000","playerLeds":32,"mode":"static"}}),
            serde_json::json!({"lightbar":{"color":"#000000","playerLeds":0,"mode":"disco"}}),
            serde_json::json!({"lightbar":{"color":"#000000","playerLeds":0,"mode":"pulse","periodMs":5}}),
            serde_json::json!({"audioPreset":{"speaker":101}}),
            serde_json::json!({"audioPreset":{"headphone":128}}),
        ] {
            let mut d = sample_profile();
            for (k, v) in bad.as_object().unwrap() {
                d["profiles"][0][k] = v.clone();
            }
            assert!(validate(d).is_err(), "{bad}");
        }
    }
    #[test]
    fn session_and_community_records_roundtrip_and_legacy_defaults_are_safe() {
        let old = validate(sample_profile()).unwrap();
        assert!(!old.profiles[0].session.enabled);
        assert!(old.profiles[0].community.reports.is_empty());
        let mut data = sample_profile();
        data["profiles"][0]["session"] = serde_json::json!({"enabled":true,"inputRoute":"xinput","audio":"off","audioGain":50,"hideHint":true});
        data["profiles"][0]["community"] = serde_json::json!({"author":"Tester","version":"2","sourceUrl":"https://example.com/profile","license":"MIT","reports":[{"platform":"windows","transport":"usb","result":"partial","date":"2026-10-28","gameVersion":"1.2","notes":"Inputs work; needs more recoil"}]});
        let encoded = serde_json::to_value(validate(data.clone()).unwrap()).unwrap();
        assert_eq!(
            encoded["profiles"][0]["community"]["reports"][0]["result"],
            "partial"
        );
        assert!(validate(encoded).is_ok());
        let mut bad = data.clone();
        bad["profiles"][0]["community"]["sourceUrl"] = "javascript:alert(1)".into();
        assert!(validate(bad).is_err());
        let mut bad = data.clone();
        bad["profiles"][0]["session"]["audioGain"] = 101.into();
        assert!(validate(bad).is_err());
        data["profiles"][0]["community"]["reports"][0]["platform"] = "all".into();
        assert!(validate(data).is_err());
    }

    #[test]
    fn backup_restore_keeps_current_archive_and_rejects_traversal() {
        let folder = env::temp_dir().join(format!(
            "controller-recovery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&folder).unwrap();
        let file = folder.join("profiles.json");
        let first = serde_json::to_vec_pretty(&validate(sample_profile()).unwrap()).unwrap();
        write_archive(&file, &first).unwrap();
        let mut second = sample_profile();
        second["profiles"][0]["name"] = "Second".into();
        let second = serde_json::to_vec_pretty(&validate(second).unwrap()).unwrap();
        save_at(&file, &second).unwrap();
        let backups = history(&file).unwrap();
        assert_eq!(backups.len(), 1);
        let restored = restore_at(&file, &backups[0].id).unwrap();
        assert_eq!(restored["profiles"][0]["name"], "Test");
        assert_eq!(history(&file).unwrap().len(), 2);
        assert!(restore_at(&file, "../profiles.json").is_err());
        assert!(restore_at(&file, "profiles-../1.json").is_err());
        // A corrupt current archive cannot be replaced by an ordinary save.
        fs::write(&file, b"corrupt").unwrap();
        assert!(save_at(&file, &first).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"corrupt");
        assert!(restore_at(&file, &backups[0].id).is_ok());
        for backup in history(&file).unwrap() {
            fs::remove_file(file.with_file_name("profile-backups").join(backup.id)).unwrap();
        }
        fs::remove_file(file).unwrap();
    }

    #[test]
    fn history_retention_is_bounded_and_automatic_snapshots_are_throttled() {
        let folder = env::temp_dir().join(format!(
            "controller-retention-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&folder).unwrap();
        let file = folder.join("profiles.json");
        write_archive(&file, &serde_json::to_vec(&sample_profile()).unwrap()).unwrap();
        for _ in 0..23 {
            snapshot(&file, true).unwrap();
        }
        assert_eq!(history(&file).unwrap().len(), 20);
        snapshot(&file, false).unwrap();
        assert_eq!(history(&file).unwrap().len(), 20);
        for backup in history(&file).unwrap() {
            fs::remove_file(file.with_file_name("profile-backups").join(backup.id)).unwrap();
        }
        fs::remove_file(file).unwrap();
    }

    fn sample_profile() -> Value {
        serde_json::json!({
            "schemaVersion": 2, "activeId": "one", "autoApply": true, "profiles": [{
                "id": "one", "name": "Test", "description": "", "tag": "GENERAL",
                "autoApply": true,
                "triggers": {
                    "left": {"mode":"resistance","strength":40,"curve":[0,0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8,0.9,1]},
                    "right":{"mode":"off","strength":0,"curve":[0,0,0,0,0,0,0,0,0,0,0]}
                },
                "haptics":{"shape":"sine","frequency":90,"attack":12,"release":220,"strength":44,"pan":0},
                "gyroAim":{"enabled":false,"sensitivity":1,"yawScale":1,"pitchScale":1,"invertY":false,"activation":"always"},
                "remap":{"enabled":false,"bindings":[]}
            }]
        })
    }

    #[test]
    fn archive_accepts_every_haptic_shape_and_rejects_unknown_ones() {
        for shape in [
            "sine", "square", "saw", "noise", "triangle", "pulse", "double", "swell",
        ] {
            let mut value = sample_profile();
            value["profiles"][0]["haptics"]["shape"] = shape.into();
            assert!(validate(value).is_ok(), "{shape} should be accepted");
        }
        let mut value = sample_profile();
        value["profiles"][0]["haptics"]["shape"] = "wobble".into();
        assert!(validate(value).is_err());
    }

    #[test]
    fn archive_rejects_out_of_range_trigger() {
        let mut value = sample_profile();
        value["profiles"][0]["triggers"]["left"]["strength"] = 101.into();
        assert!(validate(value).is_err());
    }

    #[test]
    fn motion_controls_roundtrip_and_legacy_defaults() {
        let mut value = sample_profile();
        let legacy = serde_json::to_value(validate(value.clone()).unwrap()).unwrap();
        let controls = &legacy["profiles"][0]["controls"];
        assert_eq!(controls["motion"]["countsPerTurn"], 101.25);
        assert_eq!(controls["motion"]["flickEnabled"], false);
        assert_eq!(controls["radial"]["enabled"], false);
        value["profiles"][0]["controls"] = controls.clone();
        value["profiles"][0]["controls"]["motion"]["countsPerTurn"] = 3600.into();
        value["profiles"][0]["controls"]["sticks"]["left"]["mode"] = "radial".into();
        value["profiles"][0]["controls"]["radial"]["sectors"][0]["chord"] =
            serde_json::json!([18, 9]);
        let saved = serde_json::to_value(validate(value.clone()).unwrap()).unwrap();
        assert_eq!(
            saved["profiles"][0]["controls"]["motion"]["countsPerTurn"],
            3600.0
        );
        assert_eq!(
            saved["profiles"][0]["controls"]["radial"]["sectors"][0]["chord"],
            serde_json::json!([18, 9])
        );
        value["profiles"][0]["controls"]["motion"]["countsPerTurn"] = 0.into();
        assert!(validate(value.clone()).is_err());
        value["profiles"][0]["controls"]["motion"]["countsPerTurn"] = 3600.into();
        value["profiles"][0]["controls"]["radial"]["activationButton"] = 1.into();
        assert!(validate(value).is_err());
    }

    #[test]
    fn game_actions_and_weapon_library_roundtrip_and_reject_dangling_presets() {
        let mut value = sample_profile();
        value["triggerPresets"] = serde_json::json!([{"id":"rifle","name":"My rifle","triggers":value["profiles"][0]["triggers"],"inputEffects":{}}]);
        value["profiles"][0]["actionsEnabled"] = true.into();
        value["profiles"][0]["actions"] = serde_json::json!([{"id":"roll","name":"Roll","source":"double","button":1,"durationMs":120,"haptic":{"shape":"sine","frequency":80,"strength":40}}]);
        value["profiles"][0]["weaponBindings"] =
            serde_json::json!([{"weapon":"smg","presetId":"rifle"}]);
        let roundtrip = serde_json::to_value(validate(value.clone()).unwrap()).unwrap();
        assert_eq!(roundtrip["triggerPresets"][0]["id"], "rifle");
        assert_eq!(roundtrip["profiles"][0]["actions"][0]["source"], "double");
        value["profiles"][0]["weaponBindings"][0]["presetId"] = "missing".into();
        assert!(validate(value).is_err());
    }

    #[test]
    fn custom_trigger_controls_roundtrip_and_reject_overlapping_kicks() {
        let mut value = sample_profile();
        value["profiles"][0]["inputEffects"] = serde_json::json!({
            "enabled":true,"kind":"burst","threshold":0.77,"strength":100,
            "rate":4,"spacingMs":251,"kickMs":95,"kickStyle":"return",
            "startZone":3,"endZone":7,"returnStrength":73,"hapticStrength":85,"hapticFrequency":125,"hapticDurationMs":70
        });
        value["profiles"][0]["triggers"]["left"]["preloadStrength"] = 25.into();
        value["profiles"][0]["triggers"]["left"]["frequencyHz"] = 17.into();
        let saved = serde_json::to_value(validate(value.clone()).unwrap()).unwrap();
        assert_eq!(saved["profiles"][0]["inputEffects"]["spacingMs"], 251);
        assert_eq!(saved["profiles"][0]["inputEffects"]["kickMs"], 95);
        assert_eq!(saved["profiles"][0]["inputEffects"]["kickStyle"], "return");
        assert_eq!(saved["profiles"][0]["inputEffects"]["hapticStrength"], 85);
        assert_eq!(saved["profiles"][0]["inputEffects"]["hapticFrequency"], 125);
        assert_eq!(saved["profiles"][0]["inputEffects"]["hapticDurationMs"], 70);
        assert_eq!(saved["profiles"][0]["inputEffects"]["returnStrength"], 73);
        assert_eq!(saved["profiles"][0]["inputEffects"]["startZone"], 3);
        assert_eq!(
            saved["profiles"][0]["triggers"]["left"]["preloadStrength"],
            25.0
        );
        assert_eq!(saved["profiles"][0]["triggers"]["left"]["frequencyHz"], 17);
        value["profiles"][0]["inputEffects"]["hapticStrength"] = 101.into();
        assert!(validate(value.clone()).is_err());
        value["profiles"][0]["inputEffects"]["hapticStrength"] = 85.into();
        value["profiles"][0]["inputEffects"]["kickStyle"] = "unknown".into();
        assert!(validate(value.clone()).is_err());
        value["profiles"][0]["inputEffects"]["kickStyle"] = "return".into();
        value["profiles"][0]["inputEffects"]["spacingMs"] = 100.into();
        assert!(validate(value).is_err());
    }

    #[test]
    fn archive_rejects_duplicate_remap_buttons() {
        let mut value = sample_profile();
        value["profiles"][0]["remap"]["bindings"] =
            serde_json::json!([{"button": 0, "vk": 65}, {"button": 0, "vk": 66}]);
        assert!(validate(value).is_err());
    }

    #[test]
    fn layers_allow_same_button_but_reject_unsafe_sequences() {
        let mut value = sample_profile();
        value["profiles"][0]["remap"]["bindings"] = serde_json::json!([
            {"button": 0, "vk": 65},
            {"button": 0, "vk": 66, "modifier": 4, "behavior": "macro", "sequence": [67, 68]}
        ]);
        assert!(validate(value.clone()).is_ok());
        value["profiles"][0]["remap"]["bindings"][1]["modifier"] = 0.into();
        assert!(validate(value.clone()).is_err());
        value["profiles"][0]["remap"]["bindings"][1]["modifier"] = 4.into();
        value["profiles"][0]["remap"]["bindings"][1]["sequence"] = serde_json::json!([0]);
        assert!(validate(value).is_err());
    }

    #[test]
    fn old_schemas_migrate_to_three() {
        let mut value = sample_profile();
        for schema in [1, 2] {
            value["schemaVersion"] = schema.into();
            let saved = serde_json::to_value(validate(value.clone()).unwrap()).unwrap();
            assert_eq!(saved["schemaVersion"], 3);
            assert_eq!(
                saved["profiles"][0]["remap"]["bindings"],
                serde_json::json!([])
            );
        }
    }

    #[test]
    fn executable_match_survives_frontend_roundtrip_and_legacy_name() {
        let mut value = sample_profile();
        value["profiles"][0]["matchExe"] = "game.exe".into();
        value["profiles"][0]["matchTitle"] = "Rally".into();
        value["profiles"][0]["matchPath"] = r"C:\Games\game.exe".into();
        let saved = serde_json::to_value(validate(value.clone()).unwrap()).unwrap();
        assert_eq!(saved["profiles"][0]["matchExe"], "game.exe");
        assert_eq!(saved["profiles"][0]["matchTitle"], "Rally");
        assert_eq!(saved["profiles"][0]["matchPath"], r"C:\Games\game.exe");
        value["profiles"][0]
            .as_object_mut()
            .unwrap()
            .remove("matchExe");
        value["profiles"][0]["match_exe"] = "legacy.exe".into();
        let migrated = serde_json::to_value(validate(value).unwrap()).unwrap();
        assert_eq!(migrated["profiles"][0]["matchExe"], "legacy.exe");
    }

    #[test]
    fn refined_process_match_requires_executable_name() {
        let mut value = sample_profile();
        value["profiles"][0]["matchTitle"] = "Rally".into();
        assert!(validate(value.clone()).is_err());
        value["profiles"][0]["matchExe"] = "game.exe".into();
        assert!(validate(value).is_ok());
    }

    #[test]
    fn archive_replacement_keeps_a_complete_file() {
        let path = std::env::temp_dir().join(format!(
            "aftertouch-profile-test-{}.json",
            std::process::id()
        ));
        write_archive(&path, b"first").unwrap();
        write_archive(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_archive_migrates_without_removing_original() {
        let folder = std::env::temp_dir().join(format!(
            "controller-profile-migration-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old = folder.join("Aftertouch").join("profiles.json");
        let new = folder.join("CONTROLLER").join("profiles.json");
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::write(&old, serde_json::to_vec(&sample_profile()).unwrap()).unwrap();
        let loaded = load_from_paths(&new, &old).unwrap().unwrap();
        assert_eq!(loaded["schemaVersion"], 3);
        assert!(old.exists());
        assert_eq!(
            fs::read(new).unwrap(),
            serde_json::to_vec_pretty(&loaded).unwrap()
        );
        fs::remove_dir_all(folder).unwrap();
    }
}
