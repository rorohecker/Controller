//! Keep the privileged app webview on its bundled origin.
pub fn allowed_navigation(url: &tauri::Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && match url.scheme() {
            "tauri" => url.host_str() == Some("localhost"),
            "http" | "https" => url.host_str() == Some("tauri.localhost"),
            _ => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn privileged_navigation_stays_local() {
        for value in [
            "tauri://localhost/index.html#profiles",
            "http://tauri.localhost/",
            "https://tauri.localhost/assets/icon.png",
        ] {
            assert!(
                allowed_navigation(&tauri::Url::parse(value).unwrap()),
                "{value}"
            );
        }
        for value in [
            "https://github.com/",
            "https://tauri.localhost.evil/",
            "http://tauri.localhost:8080/",
            "https://user@tauri.localhost/",
            "file:///C:/data.html",
            "data:text/html,bad",
            "javascript:alert(1)",
        ] {
            assert!(
                !allowed_navigation(&tauri::Url::parse(value).unwrap()),
                "{value}"
            );
        }
    }
}
