//! The `desktop` platform's toolkit: what the window is drawn with decides
//! which detector applies. GPUI apps get the GPUI source engine; Electron,
//! Tauri and other web-tech shells keep the web detector; anything else
//! (Qt, WinUI, AppKit, egui...) gets design review only, like iOS/Android.

use crate::jsp;
use crate::util::{exists, read_json, safe_read};
use once_cell::sync::Lazy;
use regex::Regex;

pub const GPUI: &str = "gpui";
pub const WEB: &str = "web";

/// A `gpui` (or `gpui-component`) dependency line in a Cargo manifest.
static RE_GPUI_DEP: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)^\s*(?:gpui|gpui[-_]component)\s*(?:=|\.workspace)"#).unwrap());

const WEB_SHELL_DEPENDENCIES: [&str; 5] = ["electron", "@tauri-apps/api", "@tauri-apps/cli", "nw", "@electron-forge/cli"];

/// Whether a Cargo manifest depends on GPUI.
pub fn manifest_uses_gpui(manifest: &str) -> bool {
    RE_GPUI_DEP.is_match(manifest)
}

/// Whether the project root (or its repo root) builds with GPUI: the root
/// manifest, or one crate under `crates/` (one directory level, no walk).
pub fn uses_gpui(project_root: &str, repo_root: &str) -> bool {
    let mut roots = vec![project_root];
    if !repo_root.is_empty() && repo_root != project_root {
        roots.push(repo_root);
    }
    for root in roots {
        if root.is_empty() {
            continue;
        }
        if safe_read(&jsp::join(&[root, "Cargo.toml"])).is_some_and(|m| manifest_uses_gpui(&m)) {
            return true;
        }
        let Ok(rd) = std::fs::read_dir(jsp::join(&[root, "crates"])) else { continue };
        for entry in rd.flatten() {
            let manifest = entry.path().join("Cargo.toml");
            if std::fs::read_to_string(manifest).is_ok_and(|m| manifest_uses_gpui(&m)) {
                return true;
            }
        }
    }
    false
}

fn uses_web_shell(project_root: &str) -> bool {
    if exists(&jsp::join(&[project_root, "src-tauri", "tauri.conf.json"])) {
        return true;
    }
    let Some(pkg) = read_json(&jsp::join(&[project_root, "package.json"])) else { return false };
    ["dependencies", "devDependencies"].iter().any(|k| {
        pkg.get(*k)
            .and_then(|d| d.as_object())
            .is_some_and(|d| WEB_SHELL_DEPENDENCIES.iter().any(|n| d.contains_key(*n)))
    })
}

/// The toolkit of a `desktop` project, `None` when neither GPUI nor a web
/// shell is found.
pub fn desktop_toolkit(project_root: &str, repo_root: &str) -> Option<&'static str> {
    if uses_gpui(project_root, repo_root) {
        Some(GPUI)
    } else if uses_web_shell(project_root) {
        Some(WEB)
    } else {
        None
    }
}

/// Whether `impeccable detect` and the design hook read this project's UI
/// code. Mobile native platforms never; `desktop` only with a toolkit the
/// detector knows.
pub fn detector_applies(platform: Option<&str>, toolkit: Option<&str>) -> bool {
    match platform {
        Some("ios" | "android" | "adaptive") => false,
        Some("desktop") => toolkit.is_some(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpui_manifest_lines() {
        assert!(manifest_uses_gpui("[dependencies]\ngpui = \"0.2.2\"\n"));
        assert!(manifest_uses_gpui("[dependencies]\ngpui.workspace = true\n"));
        assert!(manifest_uses_gpui("[workspace.dependencies]\n  gpui = { path = \"crates/gpui\" }\n"));
        assert!(manifest_uses_gpui("gpui-component = \"0.1\"\n"));
        assert!(!manifest_uses_gpui("[dependencies]\ngpui_macros_helper = \"1\"\nnot-gpui = \"1\"\n"));
    }

    #[test]
    fn detector_gate() {
        assert!(detector_applies(None, None));
        assert!(detector_applies(Some("web"), None));
        assert!(!detector_applies(Some("ios"), None));
        assert!(detector_applies(Some("desktop"), Some(GPUI)));
        assert!(detector_applies(Some("desktop"), Some(WEB)));
        assert!(!detector_applies(Some("desktop"), None));
    }
}
