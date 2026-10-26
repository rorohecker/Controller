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
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    id: String,
    name: String,
    description: String,
    tag: String,
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
    remap: RemapSettings,
    #[serde(default)]
    input_effects: InputEffectSettings,
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
impl Default for InputEffectSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: default_click_threshold(),
            strength: default_click_strength(),
            kind: default_weapon_kind(),
            rate: default_weapon_rate(),
        }
    }
}

fn paths() -> Result<(PathBuf, PathBuf), String> {
    let base = config_base()?;
    Ok((
        base.join("CONTROLLER").join("profiles.json"),
        base.join("Aftertouch").join("profiles.json"),
    ))
}

fn config_base() -> Result<PathBuf, String> {
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
    let mut ids = std::collections::HashSet::new();
    for p in &archive.profiles {
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
        p.triggers.left.validate().map_err(str::to_string)?;
        p.triggers.right.validate().map_err(str::to_string)?;
        let h = &p.haptics;
        if !matches!(h.shape.as_str(), "sine" | "square" | "saw" | "noise")
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
        {
            return Err("Gyro aim settings are out of range".into());
        }
        if !(0.2..=0.95).contains(&p.input_effects.threshold)
            || p.input_effects.strength > 100
            || !(3..=20).contains(&p.input_effects.rate)
            || !matches!(
                p.input_effects.kind.as_str(),
                "click" | "pistol" | "sniper" | "burst" | "smg"
            )
        {
            return Err("Input-driven trigger settings are out of range".into());
        }
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
                    || b.sequence.iter().any(|&vk| vk == 0 || vk > 255)
                    || !mapped_buttons.insert((b.button, b.modifier))
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
    write_archive(&file, &json)
}

fn write_archive(file: &std::path::Path, json: &[u8]) -> Result<(), String> {
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
    let user = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or("Home folder is unavailable")?;
    let folder = PathBuf::from(user).join("Downloads");
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
    fn archive_rejects_out_of_range_trigger() {
        let mut value = sample_profile();
        value["profiles"][0]["triggers"]["left"]["strength"] = 101.into();
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
