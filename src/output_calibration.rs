//! Per-controller, reversible software correction. No firmware feature reports.
use crate::input_map::InputCalibration;
use aftertouch_protocol::InputState;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub calibration: InputCalibration,
    pub axis_noise: [f32; 4],
    pub trigger_noise: [f32; 2],
    pub enabled: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            calibration: InputCalibration::default(),
            axis_noise: [0.0; 4],
            trigger_noise: [0.0; 2],
            enabled: true,
        }
    }
}
impl Settings {
    fn valid(&self) -> bool {
        self.calibration.valid()
            && self
                .axis_noise
                .iter()
                .all(|v| v.is_finite() && (0.0..=0.2).contains(v))
            && self
                .trigger_noise
                .iter()
                .all(|v| v.is_finite() && (0.0..=0.1).contains(v))
    }
    /// Accept raw HID values exactly once, before per-game response curves.
    pub fn apply(&self, input: &mut InputState) {
        if !self.enabled || !self.valid() {
            return;
        }
        self.calibration.apply(input);
        for (axis, noise) in input.axes.iter_mut().zip(self.axis_noise) {
            *axis = axis.signum() * ((axis.abs() - noise) / (1.0 - noise)).clamp(0.0, 1.0);
        }
        for (trigger, noise) in input.buttons[6..8].iter_mut().zip(self.trigger_noise) {
            *trigger = ((*trigger - noise) / (1.0 - noise)).clamp(0.0, 1.0);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    schema_version: u8,
    pub controller_id: String,
    pub revision: u32,
    pub settings: Settings,
}

fn valid_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) {
        return Err("Invalid controller identity".into());
    }
    Ok(())
}
fn folder() -> Result<PathBuf, String> {
    Ok(crate::profiles::config_base()?
        .join("CONTROLLER")
        .join("calibration"))
}
fn path(folder: &Path, id: &str, previous: bool) -> Result<PathBuf, String> {
    valid_id(id)?;
    let hash = id.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    Ok(folder.join(format!(
        "{hash:016x}{}.json",
        if previous { ".previous" } else { "" }
    )))
}
fn read_bytes(file: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match std::fs::symlink_metadata(file) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16_384 {
        return Err("Calibration is not a regular file or exceeds 16 KiB".into());
    }
    std::fs::read(file).map(Some).map_err(|e| e.to_string())
}
fn parse(bytes: &[u8], id: &str) -> Result<Record, String> {
    let record: Record = serde_json::from_slice(bytes).map_err(
        |_| "Calibration file is invalid. Restore the previous version or reset ranges.",
    )?;
    if record.schema_version != 1
        || record.controller_id != id
        || record.revision == 0
        || !record.settings.valid()
    {
        return Err("Calibration identity, version, or ranges are invalid".into());
    }
    Ok(record)
}
fn load_at(folder: &Path, id: &str, previous: bool) -> Result<Option<Record>, String> {
    read_bytes(&path(folder, id, previous)?)?
        .map(|bytes| parse(&bytes, id))
        .transpose()
}
pub fn load(id: &str) -> Result<Option<Record>, String> {
    load_at(&folder()?, id, false)
}

pub fn project(
    raw: &InputState,
    record: Option<&Record>,
    legacy: Option<&InputCalibration>,
    sticks: &crate::input_map::controls::StickSettings,
) -> (InputState, [f32; 4]) {
    let mut output = raw.clone();
    if let Some(record) = record {
        record.settings.apply(&mut output);
    } else if let Some(legacy) = legacy {
        legacy.apply(&mut output);
    }
    let corrected = output.axes;
    output.axes = sticks.apply(output.axes);
    (output, corrected)
}

pub fn neutral_due(
    last_valid: std::time::Instant,
    now: std::time::Instant,
    already_neutral: bool,
) -> bool {
    !already_neutral
        && now.saturating_duration_since(last_valid) >= std::time::Duration::from_millis(250)
}

// Tauri runs writes on its blocking pool, separately from the core's HID loop.
static WRITER: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn save_at(
    folder: &Path,
    id: &str,
    settings: Settings,
    expected_revision: Option<u32>,
    restore: bool,
    reset: bool,
) -> Result<Record, String> {
    valid_id(id)?;
    if !settings.valid() {
        return Err("Calibration ranges or rest noise are invalid".into());
    }
    let file = path(folder, id, false)?;
    let prior = read_bytes(&file)?;
    let current = prior.as_ref().map(|bytes| parse(bytes, id)).transpose();
    if current.is_err() && !restore && !reset {
        return Err("Restore or reset the invalid calibration before saving".into());
    }
    let revision = current
        .as_ref()
        .ok()
        .and_then(|v| v.as_ref())
        .map_or(0, |v| v.revision);
    if current.is_ok() && expected_revision != (revision != 0).then_some(revision) {
        return Err("Calibration changed. Reload before saving again.".into());
    }
    let settings = if restore {
        load_at(folder, id, true)?
            .ok_or("No previous calibration is available")?
            .settings
    } else {
        settings
    };
    let next_revision = if reset {
        revision.checked_add(1).unwrap_or(1)
    } else {
        revision
            .checked_add(1)
            .ok_or("Calibration revision limit reached; reset ranges to start again")?
    };
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    if let Some(bytes) = prior {
        // Preserve damaged data separately; never replace a valid recovery copy with it.
        let backup = if current.is_ok() && !restore {
            path(folder, id, true)?
        } else {
            file.with_extension("recovered.json")
        };
        crate::profiles::write_archive(&backup, &bytes)?;
    }
    let record = Record {
        schema_version: 1,
        controller_id: id.to_owned(),
        revision: next_revision,
        settings,
    };
    crate::profiles::write_archive(
        &file,
        &serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?,
    )?;
    Ok(record)
}

#[tauri::command]
pub async fn save_output_calibration(
    controller_id: String,
    settings: Settings,
    expected_revision: Option<u32>,
    operation: String,
) -> Result<Record, String> {
    if !matches!(operation.as_str(), "save" | "restore" | "reset") {
        return Err("Unknown calibration operation".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = WRITER.lock().map_err(|_| "Calibration storage is busy")?;
        save_at(
            &folder()?,
            &controller_id,
            settings,
            expected_revision,
            operation == "restore",
            operation == "reset",
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> InputState {
        aftertouch_protocol::parse_input(&{
            let mut b = [0u8; 64];
            b[0] = 1;
            b[1..5].fill(128);
            b[8] = 8;
            b
        })
        .unwrap()
    }
    #[test]
    fn correction_is_centered_bounded_and_reversible() {
        let mut settings = Settings::default();
        settings.calibration.axis_centers = [0.1; 4];
        settings.calibration.axis_mins = [-0.8; 4];
        settings.calibration.axis_maxs = [0.9; 4];
        settings.axis_noise = [0.02; 4];
        settings.trigger_noise = [0.02; 2];
        let mut input = sample();
        input.axes = [0.11, 0.1, -0.8, 0.9];
        input.buttons[6] = 0.01;
        input.buttons[7] = 1.0;
        settings.apply(&mut input);
        assert_eq!(input.axes, [0.0, 0.0, -1.0, 1.0]);
        assert_eq!(&input.buttons[6..8], &[0.0, 1.0]);
        settings.enabled = false;
        let mut input = sample();
        input.axes = [0.1; 4];
        settings.apply(&mut input);
        assert_eq!(input.axes, [0.1; 4]);
    }
    #[test]
    fn saved_ranges_survive_restart_with_identity_conflict_and_recovery_guards() {
        let folder = std::env::temp_dir().join(format!(
            "controller-cal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = save_at(&folder, "pad-a", Settings::default(), None, false, false).unwrap();
        let mut second = first.settings.clone();
        second.axis_noise[0] = 0.05;
        save_at(&folder, "pad-a", second, Some(1), false, false).unwrap();
        assert_eq!(
            load_at(&folder, "pad-a", false)
                .unwrap()
                .unwrap()
                .settings
                .axis_noise[0],
            0.05
        );
        assert!(load_at(&folder, "pad-b", false).unwrap().is_none());
        assert!(
            save_at(
                &folder,
                "pad-a",
                first.settings.clone(),
                Some(1),
                false,
                false
            )
            .is_err()
        );
        std::fs::write(path(&folder, "pad-a", false).unwrap(), b"broken").unwrap();
        assert!(load_at(&folder, "pad-a", false).is_err());
        assert!(save_at(&folder, "pad-a", Settings::default(), None, false, false).is_err());
        let restored = save_at(&folder, "pad-a", Settings::default(), None, true, false).unwrap();
        assert_eq!(restored.settings, first.settings);
        let loaded = load_at(&folder, "pad-a", false).unwrap().unwrap();
        assert_eq!(loaded.settings, first.settings);
        let mut exhausted = loaded;
        exhausted.revision = u32::MAX;
        std::fs::write(
            path(&folder, "pad-a", false).unwrap(),
            serde_json::to_vec(&exhausted).unwrap(),
        )
        .unwrap();
        let prior_backup = read_bytes(&path(&folder, "pad-a", true).unwrap()).unwrap();
        assert!(
            save_at(
                &folder,
                "pad-a",
                Settings::default(),
                Some(u32::MAX),
                false,
                false
            )
            .is_err()
        );
        assert_eq!(
            read_bytes(&path(&folder, "pad-a", true).unwrap()).unwrap(),
            prior_backup
        );
        assert_eq!(
            save_at(
                &folder,
                "pad-a",
                Settings::default(),
                Some(u32::MAX),
                false,
                true
            )
            .unwrap()
            .revision,
            1
        );
        for file in std::fs::read_dir(&folder).unwrap() {
            std::fs::remove_file(file.unwrap().path()).unwrap();
        }
        std::fs::remove_dir(folder).unwrap();
    }
    #[test]
    fn rejects_untrusted_ranges_and_record_identity() {
        let mut settings = Settings::default();
        settings.axis_noise[0] = f32::NAN;
        assert!(!settings.valid());
        settings.axis_noise[0] = 0.21;
        assert!(!settings.valid());
        assert!(valid_id("").is_err());
        assert!(valid_id("bad\0id").is_err());
        let record = Record {
            schema_version: 1,
            controller_id: "a".into(),
            revision: 1,
            settings: Settings::default(),
        };
        assert!(parse(&serde_json::to_vec(&record).unwrap(), "b").is_err());
    }
    #[test]
    fn corrected_values_are_encoded_once_at_the_virtual_gamepad_boundary() {
        let mut settings = Settings::default();
        settings.calibration.axis_centers = [0.1; 4];
        settings.calibration.axis_mins = [-0.8; 4];
        settings.calibration.axis_maxs = [0.9; 4];
        settings.calibration.trigger_rests = [0.1; 2];
        settings.calibration.trigger_fulls = [0.9; 2];
        let mut raw = sample();
        raw.axes = [0.3, 0.1, -0.8, 0.9];
        raw.buttons[6] = 0.3;
        raw.buttons[7] = 0.9;
        raw.buttons[0] = 1.0;
        raw.buttons[12] = 1.0;
        raw.buttons[16] = 1.0;
        let mut curves = crate::input_map::controls::StickSettings::default();
        curves.left.inner = 0.0;
        curves.right.inner = 0.0;
        let record = Record {
            schema_version: 1,
            controller_id: "a".into(),
            revision: 1,
            settings,
        };
        let (corrected, _) = project(
            &raw,
            Some(&record),
            Some(&record.settings.calibration),
            &curves,
        );
        let encoded = crate::xinput_bridge::Encoded::from_input(&corrected);
        assert_eq!(encoded.thumb_lx, 8192);
        assert_eq!(encoded.thumb_ly, 0);
        assert_eq!(encoded.thumb_rx, -32768);
        assert_eq!(encoded.thumb_ry, -32768);
        assert_eq!(encoded.left_trigger, 64);
        assert_eq!(encoded.right_trigger, 255);
        assert_eq!(encoded.buttons, 0x1401);
        // The source report remains untouched, including while a UI measurement uses defaults.
        assert_eq!(raw.axes, [0.3, 0.1, -0.8, 0.9]);
        assert_eq!(raw.buttons[6], 0.3);
        let (legacy, _) = project(&raw, None, Some(&record.settings.calibration), &curves);
        assert_eq!(
            crate::xinput_bridge::Encoded::from_input(&legacy).thumb_lx,
            8192
        );
        let (measurement, _) = project(&raw, None, None, &curves);
        assert_eq!(
            crate::xinput_bridge::Encoded::from_input(&measurement).thumb_lx,
            9830
        );
        let mut bypass = record.clone();
        bypass.settings.enabled = false;
        let (bypassed, _) = project(
            &raw,
            Some(&bypass),
            Some(&record.settings.calibration),
            &curves,
        );
        assert_eq!(
            crate::xinput_bridge::Encoded::from_input(&bypassed).thumb_lx,
            9830
        );
    }
    #[test]
    fn neutral_watchdog_fires_once_then_recovers_on_fresh_input() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        assert!(!neutral_due(
            start,
            start + Duration::from_millis(249),
            false
        ));
        assert!(neutral_due(
            start,
            start + Duration::from_millis(250),
            false
        ));
        assert!(!neutral_due(start, start + Duration::from_secs(4), true));
        assert!(!neutral_due(
            start + Duration::from_secs(4),
            start + Duration::from_secs(4),
            false
        ));
    }
}
