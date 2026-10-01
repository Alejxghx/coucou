// Agent-specific hook installation. Configuration changes are previewed first,
// backed up, and applied only after an explicit click in the settings window.

use std::path::PathBuf;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use windows::Win32::System::SystemInformation::GetLocalTime;
use crate::{hooks_config::{config_path, HookConfig}, settings};
pub use crate::hooks_config::{HookAgent, HookPreview};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStatus {
    pub installed: bool,
    pub settings_path: String,
    pub hook_path: String,
    pub hook_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn config(agent: HookAgent) -> Result<HookConfig, String> {
    let path = config_path(agent,
        std::env::var_os("USERPROFILE").map(PathBuf::from),
        std::env::var_os("CODEX_HOME").map(PathBuf::from),
    )?;
    Ok(HookConfig { agent, path, hook_path: settings::hook_exe_path() })
}

fn stamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

pub fn status(agent: HookAgent) -> HookStatus {
    let config = config(agent);
    let hook_path = settings::hook_exe_path();
    let result = config.as_ref().map_err(Clone::clone).and_then(HookConfig::installed);
    HookStatus {
        installed: result.as_ref().copied().unwrap_or(false),
        settings_path: config.as_ref().map(|config| config.path.to_string_lossy().into_owned()).unwrap_or_default(),
        hook_path: hook_path.to_string_lossy().into_owned(),
        hook_ready: hook_path.is_file(),
        error: result.err(),
    }
}

pub fn preview(install: bool, agent: HookAgent) -> Result<HookPreview, String> {
    config(agent)?.preview(install, &stamp())
}

pub fn write(install: bool, fingerprint: &str, agent: HookAgent) -> Result<String, String> {
    config(agent)?.write(install, fingerprint, &stamp())
}

/// Copies coucou-hook.exe into %LOCALAPPDATA%\Coucou\bin on launch.
/// In a bundled install it comes from the app resources; in `tauri dev` it sits
/// next to coucou.exe in the workspace target directory.
///
/// Every candidate is tried rather than just the first, because getting this
/// wrong is silent and fatal: `resources` used to be a glob, which made NSIS
/// mirror the source path into `_up_\target\release\`, no candidate matched, and
/// the relay was simply never installed. It only looked healthy on a developer
/// machine, where a leftover copy from `tauri dev` was already sitting in bin/.
pub fn ensure_hook_exe(app: &AppHandle) {
    let dest = settings::hook_exe_path();
    let Some(dir) = dest.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(p) = app.path().resolve("coucou-hook.exe", tauri::path::BaseDirectory::Resource) {
        candidates.push(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            // Installed build, then `tauri dev` (target/debug) next to the
            // release hook the pre-build step produces.
            candidates.push(parent.join("coucou-hook.exe"));
            candidates.push(parent.join("../release/coucou-hook.exe"));
            // Belt and braces: where the old glob form used to land it.
            candidates.push(parent.join("_up_/target/release/coucou-hook.exe"));
        }
    }

    let tried: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
    let Some(src) = candidates.into_iter().find(|p| p.exists()) else {
        crate::log::line(format!(
            "coucou-hook.exe not found — agent hooks cannot work. Looked in: {}",
            tried.join(", ")
        ));
        return;
    };

    let same = match (std::fs::metadata(&src), std::fs::metadata(&dest)) {
        (Ok(a), Ok(b)) => a.len() == b.len() && a.modified().ok() == b.modified().ok(),
        _ => false,
    };
    if same {
        return;
    }
    // A hook may be running right now and hold the file open; keeping the old
    // copy is fine, it is the same relay.
    if let Err(err) = std::fs::copy(&src, &dest) {
        if !dest.exists() {
            crate::log::line(format!("could not install coucou-hook.exe: {err}"));
        }
    }
}
