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
    #[serde(default = "default_true")]
    auto_apply: bool,
    triggers: TriggerPair,
    haptics: Haptics,
    #[serde(default)]
    gyro_aim: GyroAimSettings,
    #[serde(default)]
    remap: RemapSettings,
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

impl Default for GyroAimSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            sensitivity: 1.0,
            yaw_scale: 1.0,
            pitch_scale: 1.0,
            invert_y: false,
            activation: "always".into(),
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
}

fn path() -> Result<PathBuf, String> {
    let base = env::var_os("APPDATA").ok_or("APPDATA is unavailable")?;
    Ok(PathBuf::from(base).join("Aftertouch").join("profiles.json"))
}

fn validate(data: Value) -> Result<Archive, String> {
    let mut archive: Archive = serde_json::from_value(data).map_err(|e| e.to_string())?;
    // Accept schema 1 (legacy) and 2 (auto-apply / gyro / remap fields).
    if archive.schema_version == 1 {
        archive.schema_version = 2;
    }
    if archive.schema_version != 2 || archive.profiles.is_empty() || archive.profiles.len() > 50 {
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
        {
            return Err("Gyro aim settings are out of range".into());
        }
        if p.remap.bindings.len() > 23
            || p.remap
                .bindings
                .iter()
                .any(|b| b.button >= 23 || b.vk == 0)
        {
            return Err("Remap bindings are invalid".into());
        }
    }
    Ok(archive)
}

pub fn load() -> Result<Option<Value>, String> {
    let path = path()?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    if bytes.len() > 1_000_000 {
        return Err("Profile archive is too large".into());
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let archive = validate(data)?;
    Ok(Some(
        serde_json::to_value(archive).map_err(|e| e.to_string())?,
    ))
}

pub fn save(data: Value) -> Result<(), String> {
    let archive = validate(data)?;
    let json = serde_json::to_vec_pretty(&archive).map_err(|e| e.to_string())?;
    if json.len() > 1_000_000 {
        return Err("Profile archive is too large".into());
    }
    let file = path()?;
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
    let user = env::var_os("USERPROFILE").ok_or("USERPROFILE is unavailable")?;
    let folder = PathBuf::from(user).join("Downloads");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    for suffix in 0..100 {
        let name = if suffix == 0 {
            format!("aftertouch-profiles-{stamp}.json")
        } else {
            format!("aftertouch-profiles-{stamp}-{suffix}.json")
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
    fn schema_one_migrates_to_two() {
        let mut value = sample_profile();
        value["schemaVersion"] = 1.into();
        let saved = serde_json::to_value(validate(value).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 2);
    }

    #[test]
    fn executable_match_survives_frontend_roundtrip_and_legacy_name() {
        let mut value = sample_profile();
        value["profiles"][0]["matchExe"] = "game.exe".into();
        let saved = serde_json::to_value(validate(value.clone()).unwrap()).unwrap();
        assert_eq!(saved["profiles"][0]["matchExe"], "game.exe");
        value["profiles"][0]
            .as_object_mut()
            .unwrap()
            .remove("matchExe");
        value["profiles"][0]["match_exe"] = "legacy.exe".into();
        let migrated = serde_json::to_value(validate(value).unwrap()).unwrap();
        assert_eq!(migrated["profiles"][0]["matchExe"], "legacy.exe");
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
}
