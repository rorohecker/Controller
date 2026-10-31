//! Optional Xbox 360 / XInput virtual pad via ViGEmBus (DualSenseX-style).
//! Requires the separately installed [ViGEmBus](https://github.com/nefarius/ViGEmBus) driver.
//! The Windows installer bundles its signed setup; users launch it explicitly for XInput.

#[cfg(target_os = "linux")]
use crate::virtual_gamepad::linux::Pad as NativePad;
use aftertouch_protocol::InputState;
use serde_json::{Value, json};
#[cfg(windows)]
use windows_impl::Pad as NativePad;

/// The values sent to ViGEm, also available for hardware-free verification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Encoded {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}
impl Encoded {
    pub fn from_input(input: &InputState) -> Self {
        let masks = [
            (0, 0x1000),
            (1, 0x2000),
            (2, 0x4000),
            (3, 0x8000),
            (4, 0x100),
            (5, 0x200),
            (8, 0x20),
            (9, 0x10),
            (10, 0x40),
            (11, 0x80),
            (12, 1),
            (13, 2),
            (14, 4),
            (15, 8),
            (16, 0x400),
        ];
        let buttons = masks
            .iter()
            .filter(|(i, _)| input.buttons[*i] > 0.5)
            .fold(0, |bits, (_, mask)| bits | mask);
        fn thumb(v: f32) -> i16 {
            if !v.is_finite() {
                return 0;
            }
            (v.clamp(-1.0, 1.0) * if v < 0.0 { 32768.0 } else { 32767.0 }).round() as i16
        }
        Self {
            buttons,
            left_trigger: (input.buttons[6].clamp(0.0, 1.0) * 255.0).round() as u8,
            right_trigger: (input.buttons[7].clamp(0.0, 1.0) * 255.0).round() as u8,
            thumb_lx: thumb(input.axes[0]),
            thumb_ly: thumb(-input.axes[1]),
            thumb_rx: thumb(input.axes[2]),
            thumb_ry: thumb(-input.axes[3]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XInputStatus {
    Disabled,
    Unavailable,
    Active,
}

impl XInputStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Unavailable => "unavailable",
            Self::Active => "active",
        }
    }
}

pub struct XInputBridge {
    enabled: bool,
    status: XInputStatus,
    last_error: Option<String>,
    #[cfg(any(windows, target_os = "linux"))]
    inner: Option<NativePad>,
}

impl Default for XInputBridge {
    fn default() -> Self {
        Self {
            enabled: false,
            status: XInputStatus::Disabled,
            last_error: None,
            #[cfg(any(windows, target_os = "linux"))]
            inner: None,
        }
    }
}

impl XInputBridge {
    pub fn active(&self) -> bool {
        self.status == XInputStatus::Active
    }
    pub fn neutralize(&mut self) -> Option<Value> {
        #[cfg(any(windows, target_os = "linux"))]
        if let Some(pad) = self.inner.as_mut()
            && let Err(error) = pad.neutralize()
        {
            self.shutdown();
            self.status = XInputStatus::Unavailable;
            self.last_error = Some(error);
            return Some(self.status_event());
        }
        None
    }
    /// Nonblocking notification polling. Only the hardware loop writes outputs.
    pub fn take_feedback(&mut self) -> Result<Option<[u8; 2]>, String> {
        #[cfg(any(windows, target_os = "linux"))]
        if let Some(pad) = self.inner.as_mut() {
            let result = pad.feedback();
            if let Err(error) = &result {
                self.shutdown();
                self.status = XInputStatus::Unavailable;
                self.last_error = Some(error.clone());
            }
            return result;
        }
        Ok(None)
    }

    pub fn status_event(&self) -> Value {
        json!({
            "type": "xinput",
            "enabled": self.enabled,
            "status": self.status.as_str(),
            "available": self.status == XInputStatus::Active,
            "platformSupported": cfg!(any(windows,target_os="linux")),
            "backend":if cfg!(windows){"vigem"}else if cfg!(target_os="linux"){"uinput"}else{"unavailable"},
            "message": self.status_message(),
            "error": self.last_error,
        })
    }

    fn status_message(&self) -> String {
        match self.status {
            XInputStatus::Disabled => "Virtual gamepad off".into(),
            XInputStatus::Unavailable => self.last_error.clone().unwrap_or_else(|| {
                if cfg!(windows) {
                    "ViGEmBus not found — install the Nefarius driver to enable XInput".into()
                } else {
                    "Virtual gamepad output is not available on this platform".into()
                }
            }),
            XInputStatus::Active => if cfg!(target_os = "linux") {
                "Virtual Xbox gamepad active (uinput)"
            } else {
                "Virtual Xbox 360 pad active (ViGEm)"
            }
            .into(),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) -> Value {
        if enabled && self.status == XInputStatus::Active {
            return self.status_event();
        }
        self.enabled = enabled;
        if !enabled {
            self.shutdown();
            self.status = XInputStatus::Disabled;
            self.last_error = None;
            return self.status_event();
        }
        #[cfg(any(windows, target_os = "linux"))]
        {
            match NativePad::open() {
                Ok(pad) => {
                    self.inner = Some(pad);
                    self.status = XInputStatus::Active;
                    self.last_error = None;
                }
                Err(error) => {
                    self.inner = None;
                    self.enabled = false;
                    self.status = XInputStatus::Unavailable;
                    self.last_error = Some(error);
                }
            }
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            self.enabled = false;
            self.status = XInputStatus::Unavailable;
            self.last_error =
                Some("Virtual gamepad output is not available on this platform".into());
        }
        self.status_event()
    }

    /// Returns a status event when the bridge drops due to a write failure.
    pub fn update(&mut self, input: &InputState) -> Option<Value> {
        if self.status != XInputStatus::Active {
            return None;
        }
        #[cfg(any(windows, target_os = "linux"))]
        if let Some(pad) = self.inner.as_mut()
            && let Err(error) = pad.update(input)
        {
            self.last_error = Some(error);
            self.shutdown();
            self.status = XInputStatus::Unavailable;
            return Some(self.status_event());
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = input;
        }
        None
    }

    pub fn shutdown(&mut self) {
        #[cfg(any(windows, target_os = "linux"))]
        {
            self.inner = None;
        }
        self.enabled = false;
        if self.status == XInputStatus::Active {
            self.status = XInputStatus::Disabled;
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::Encoded;
    use aftertouch_protocol::InputState;
    use std::pin::Pin;
    use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

    pub struct Pad {
        target: Xbox360Wired<Client>,
        notification: Pin<Box<vigem_client::XRequestNotification>>,
    }

    impl Pad {
        pub fn open() -> Result<Self, String> {
            let client = Client::connect().map_err(|error| {
                format!("ViGEmBus unavailable ({error}). Install Nefarius ViGEmBus, then retry.")
            })?;
            let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
            target
                .plugin()
                .map_err(|error| format!("Could not plug virtual Xbox pad: {error}"))?;
            target
                .wait_ready()
                .map_err(|error| format!("Virtual Xbox pad not ready: {error}"))?;
            let mut notification = Box::pin(
                target
                    .request_notification()
                    .map_err(|error| format!("Game feedback unavailable: {error}"))?,
            );
            notification.as_mut().request();
            Ok(Self {
                target,
                notification,
            })
        }

        pub fn feedback(&mut self) -> Result<Option<[u8; 2]>, String> {
            match self.notification.as_mut().poll(false) {
                Ok(Some(data)) => {
                    self.notification.as_mut().request();
                    Ok(Some([data.large_motor, data.small_motor]))
                }
                Ok(None) => Ok(None),
                Err(error) => Err(format!("Game feedback lost: {error}")),
            }
        }

        pub fn update(&mut self, input: &InputState) -> Result<(), String> {
            let gamepad = map_gamepad(input);
            self.target
                .update(&gamepad)
                .map_err(|error| format!("XInput update failed: {error}"))
        }
        pub fn neutralize(&mut self) -> Result<(), String> {
            self.target
                .update(&XGamepad::default())
                .map_err(|error| format!("Could not neutralize XInput: {error}"))
        }
    }

    fn map_gamepad(input: &InputState) -> XGamepad {
        let encoded = Encoded::from_input(input);
        XGamepad {
            buttons: XButtons {
                raw: encoded.buttons,
            },
            left_trigger: encoded.left_trigger,
            right_trigger: encoded.right_trigger,
            thumb_lx: encoded.thumb_lx,
            thumb_ly: encoded.thumb_ly,
            thumb_rx: encoded.thumb_rx,
            thumb_ry: encoded.thumb_ry,
        }
    }
}
