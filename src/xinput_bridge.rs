//! Optional Xbox 360 / XInput virtual pad via ViGEmBus (DualSenseX-style).
//! Requires the separately installed [ViGEmBus](https://github.com/nefarius/ViGEmBus) driver.
//! The Windows installer bundles its signed setup; users launch it explicitly for XInput.

use aftertouch_protocol::InputState;
use serde_json::{Value, json};

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
    #[cfg(windows)]
    inner: Option<windows_impl::Pad>,
}

impl Default for XInputBridge {
    fn default() -> Self {
        Self {
            enabled: false,
            status: XInputStatus::Disabled,
            last_error: None,
            #[cfg(windows)]
            inner: None,
        }
    }
}

impl XInputBridge {
    pub fn active(&self) -> bool {
        self.status == XInputStatus::Active
    }
    /// Nonblocking notification polling. Only the hardware loop writes outputs.
    pub fn take_feedback(&mut self) -> Result<Option<[u8; 2]>, String> {
        #[cfg(windows)]
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
            "platformSupported": cfg!(windows),
            "message": self.status_message(),
            "error": self.last_error,
        })
    }

    fn status_message(&self) -> String {
        match self.status {
            XInputStatus::Disabled => "XInput bridge off".into(),
            XInputStatus::Unavailable => self.last_error.clone().unwrap_or_else(|| {
                if cfg!(windows) {
                    "ViGEmBus not found — install the Nefarius driver to enable XInput".into()
                } else {
                    "XInput bridge is Windows-only".into()
                }
            }),
            XInputStatus::Active => "Virtual Xbox 360 pad active (ViGEm)".into(),
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
        #[cfg(windows)]
        {
            match windows_impl::Pad::open() {
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
        #[cfg(not(windows))]
        {
            self.enabled = false;
            self.status = XInputStatus::Unavailable;
            self.last_error = Some("XInput bridge requires Windows + ViGEmBus".into());
        }
        self.status_event()
    }

    /// Returns a status event when the bridge drops due to a write failure.
    pub fn update(&mut self, input: &InputState) -> Option<Value> {
        if self.status != XInputStatus::Active {
            return None;
        }
        #[cfg(windows)]
        if let Some(pad) = self.inner.as_mut()
            && let Err(error) = pad.update(input)
        {
            self.last_error = Some(error);
            self.shutdown();
            self.status = XInputStatus::Unavailable;
            return Some(self.status_event());
        }
        #[cfg(not(windows))]
        {
            let _ = input;
        }
        None
    }

    pub fn shutdown(&mut self) {
        #[cfg(windows)]
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
    }

    fn axis_to_thumb(value: f32) -> i16 {
        let rescaled = value.clamp(-1.0, 1.0);
        if rescaled < 0.0 {
            (rescaled * 32768.0).round() as i16
        } else {
            (rescaled * 32767.0).round() as i16
        }
    }

    fn map_gamepad(input: &InputState) -> XGamepad {
        let b = &input.buttons;
        let mut raw = 0u16;
        // DualSense → Xbox: Cross=A, Circle=B, Square=X, Triangle=Y
        if b[0] > 0.5 {
            raw |= XButtons::A;
        }
        if b[1] > 0.5 {
            raw |= XButtons::B;
        }
        if b[2] > 0.5 {
            raw |= XButtons::X;
        }
        if b[3] > 0.5 {
            raw |= XButtons::Y;
        }
        if b[4] > 0.5 {
            raw |= XButtons::LB;
        }
        if b[5] > 0.5 {
            raw |= XButtons::RB;
        }
        if b[8] > 0.5 {
            raw |= XButtons::BACK;
        }
        if b[9] > 0.5 {
            raw |= XButtons::START;
        }
        if b[10] > 0.5 {
            raw |= XButtons::LTHUMB;
        }
        if b[11] > 0.5 {
            raw |= XButtons::RTHUMB;
        }
        if b[12] > 0.5 {
            raw |= XButtons::UP;
        }
        if b[13] > 0.5 {
            raw |= XButtons::DOWN;
        }
        if b[14] > 0.5 {
            raw |= XButtons::LEFT;
        }
        if b[15] > 0.5 {
            raw |= XButtons::RIGHT;
        }
        if b[16] > 0.5 {
            raw |= XButtons::GUIDE;
        }
        XGamepad {
            buttons: XButtons { raw },
            left_trigger: (b[6].clamp(0.0, 1.0) * 255.0).round() as u8,
            right_trigger: (b[7].clamp(0.0, 1.0) * 255.0).round() as u8,
            thumb_lx: axis_to_thumb(input.axes[0]),
            // XInput Y is inverted relative to DualSense HID.
            thumb_ly: axis_to_thumb(-input.axes[1]),
            thumb_rx: axis_to_thumb(input.axes[2]),
            thumb_ry: axis_to_thumb(-input.axes[3]),
        }
    }
}
