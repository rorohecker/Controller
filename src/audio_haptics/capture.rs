//! Capture selection is explicit: a failed process capture never broadens to system audio.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CaptureSource {
    #[default]
    System,
    Process {
        pid: u32,
    },
    NativeEndpoint {
        id: String,
    },
}
impl CaptureSource {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Process { pid }
                if *pid <= 4
                    || *pid == std::process::id()
                    || std::env::var("CONTROLLER_GUI_PID")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        == Some(*pid) =>
            {
                Err("Choose a running game or app, not CONTROLLER or a system process".into())
            }
            Self::NativeEndpoint { id }
                if id.trim().is_empty() || id.len() > 2048 || id.contains(char::is_control) =>
            {
                Err("Choose a valid four-channel source endpoint".into())
            }
            _ => Ok(()),
        }
    }
}
#[cfg(windows)]
#[path = "process_capture.rs"]
pub(super) mod process;

pub(super) enum CaptureHandle {
    System {
        _worker: super::io_runtime::CaptureWorker,
    },
    #[cfg(windows)]
    Process { _worker: process::ProcessCapture },
}

impl CaptureHandle {
    pub(super) fn timed_out(&self) -> bool {
        match self {
            Self::System { _worker } => _worker.timed_out(),
            #[cfg(windows)]
            Self::Process { _worker } => _worker.timed_out(),
        }
    }
    pub(super) fn state(&self) -> &'static str {
        match self {
            Self::System { _worker } => _worker.state(),
            #[cfg(windows)]
            Self::Process { _worker } => _worker.state(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_targets_are_explicit_and_bounded() {
        assert_eq!(
            serde_json::from_str::<CaptureSource>(r#"{"kind":"system"}"#).unwrap(),
            CaptureSource::System
        );
        assert!(CaptureSource::Process { pid: 0 }.validate().is_err());
        assert!(
            CaptureSource::Process {
                pid: std::process::id()
            }
            .validate()
            .is_err()
        );
        assert!(
            CaptureSource::NativeEndpoint { id: "\n".into() }
                .validate()
                .is_err()
        );
        assert!(serde_json::from_str::<CaptureSource>(r#"{"kind":"process","pid":-1}"#).is_err());
    }
}
