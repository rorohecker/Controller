"""Unit test for tests/command_schema.py (no browser): the mock core must reject what serde would reject.

Reproduces the audit's old mistakes (B1 controller_id vs controllerId, B2 yaw_scale vs yawScale, B3 a dropped
`enabled`) against a fixture that has the OLD Rust definitions, then checks the real src/device.rs schema."""
import command_schema as cs

OLD_RUST = '''
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum DeviceCommand {
    ReloadOutputCalibration {
        controller_id: String,
    },
    SetGyroAim {
        enabled: bool,
        #[serde(default = "default_sens")]
        sensitivity: f32,
        #[serde(default = "default_axis")]
        yaw_scale: f32,
        #[serde(default = "default_axis")]
        pitch_scale: f32,
        #[serde(default)]
        invert_y: bool,
    },
    SetRuntimeDiagnostics {
        enabled: bool,
    },
    SetLightbar {
        rgb: [u8; 3],
        #[serde(default)]
        note: Option<String>,
    },
    SetPadExtras {
        #[serde(flatten)]
        extras: crate::virtual_gamepad::PadExtras,
    },
    Reset,
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PadExtras {
    pub touchpad_click: Option<PadButton>,
    pub mic: Option<PadButton>,
}
'''

FIXED_RUST = OLD_RUST.replace('        controller_id: String,', '        #[serde(rename = "controllerId", alias = "controller_id")]\n        controller_id: String,').replace(
    '        #[serde(default = "default_axis")]\n        yaw_scale: f32,', '        #[serde(default = "default_axis", rename = "yawScale", alias = "yaw_scale")]\n        yaw_scale: f32,').replace(
    '        #[serde(default)]\n        invert_y: bool,', '        #[serde(default, rename = "invertY", alias = "invert_y")]\n        invert_y: bool,')


def has(problems, text):
    return any(text in p for p in problems)


def main():
    old = cs.parse(OLD_RUST)
    # B1: the UI sent controllerId, the old Rust variant only knew controller_id.
    problems = cs.validate(old, {'action': 'reloadOutputCalibration', 'controllerId': 'pad-1'})
    assert has(problems, 'missing field `controller_id`') and has(problems, 'unknown field `controllerId`'), problems
    assert cs.validate(old, {'action': 'reloadOutputCalibration', 'controller_id': 'pad-1'}) == []
    # B2: camelCase gyro fields were silently dropped.
    problems = cs.validate(old, {'action': 'setGyroAim', 'enabled': True, 'sensitivity': 1, 'yawScale': 1, 'pitchScale': 1, 'invertY': True})
    assert [p for p in problems if 'unknown field' in p] and has(problems, '`invertY`') and has(problems, '`yawScale`'), problems
    # B3: JSON.stringify drops `enabled: undefined`.
    assert has(cs.validate(old, {'action': 'setRuntimeDiagnostics'}), 'missing field `enabled`')
    # Unknown actions and non-objects.
    assert has(cs.validate(old, {'action': 'doesNotExist'}), 'unknown action')
    assert cs.validate(old, 'nope') and cs.validate(old, {})
    # Optional fields and defaults may be omitted; required ones may not.
    assert cs.validate(old, {'action': 'setLightbar', 'rgb': [1, 2, 3]}) == []
    assert has(cs.validate(old, {'action': 'setLightbar'}), 'missing field `rgb`')
    # The persistent flag is allowed on any command.
    assert cs.validate(old, {'action': 'reset', 'persistent': True}) == []
    # A flattened struct contributes its own (camelCased) fields.
    assert cs.validate(old, {'action': 'setPadExtras', 'touchpadClick': 'a', 'mic': None}) == []
    assert has(cs.validate(old, {'action': 'setPadExtras', 'touchpad_click': 'a'}), 'unknown field')

    # The fixed definitions (rename plus snake_case alias) accept both spellings.
    fixed = cs.parse(FIXED_RUST)
    assert cs.validate(fixed, {'action': 'reloadOutputCalibration', 'controllerId': 'pad-1'}) == []
    assert cs.validate(fixed, {'action': 'reloadOutputCalibration', 'controller_id': 'pad-1'}) == []
    assert not has(cs.validate(fixed, {'action': 'setGyroAim', 'enabled': True, 'yawScale': 1, 'invertY': True}), 'unknown field `yawScale`')

    # The real contract in src/device.rs.
    real = cs.load()
    assert len(real) > 70, len(real)
    for action in ('reloadOutputCalibration', 'setGyroAim', 'setRuntimeDiagnostics', 'setPadExtras', 'applyTriggers', 'playHaptic'):
        assert action in real, action
    assert cs.validate(real, {'action': 'reloadOutputCalibration', 'controllerId': 'pad-1'}) == []
    assert cs.validate(real, {'action': 'setGyroAim', 'enabled': True, 'sensitivity': 1, 'yawScale': 1, 'pitchScale': 1, 'invertY': False}) == []
    assert has(cs.validate(real, {'action': 'setRuntimeDiagnostics'}), 'missing field `enabled`')
    assert has(cs.validate(real, {'action': 'playHaptic', 'shape': 'sine'}), 'missing field')
    assert has(cs.validate(real, {'action': 'setNothing'}), 'unknown action')
    print(f'Command schema smoke passed: {len(real)} actions, B1/B2/B3 reproduced and caught')


if __name__ == '__main__':
    main()
