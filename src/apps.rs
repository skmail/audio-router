use crate::audio::{self, Result};
use objc2_app_kit::NSWorkspace;
use objc2_core_audio::*;
use std::{collections::BTreeMap, ffi::c_void};

unsafe extern "C" {
    fn proc_pidpath(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
}

#[derive(Clone, Debug)]
pub struct App {
    pub key: String,
    pub name: String,
    pub processes: Vec<u32>,
    pub playing: bool,
}

fn process_path(pid: i32) -> Option<String> {
    let mut buffer = [0u8; 4096];
    let count = unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if count <= 0 {
        return None;
    }
    let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
    Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
}

fn belongs_to(path: &str, bundle: &str) -> bool {
    path.strip_prefix(bundle)
        .is_some_and(|rest| rest.starts_with('/'))
}

pub fn discover() -> Result<Vec<App>> {
    let running = NSWorkspace::sharedWorkspace().runningApplications();
    let mut owners = vec![];
    for app in running {
        if let (Some(bundle), Some(key), Some(name)) = (
            app.bundleURL().and_then(|u| u.path()),
            app.bundleIdentifier(),
            app.localizedName(),
        ) {
            owners.push((
                bundle.to_string(),
                key.to_string(),
                name.to_string(),
                app.processIdentifier(),
            ));
        }
    }
    // Outermost app wins over nested Chromium Helper.app bundles.
    owners.sort_by_key(|owner| owner.0.len());
    let mut groups: BTreeMap<String, App> = BTreeMap::new();
    for process in audio::ids(
        kAudioObjectSystemObject as u32,
        kAudioHardwarePropertyProcessObjectList,
        kAudioObjectPropertyScopeGlobal,
    )? {
        let Ok(pid) = audio::scalar::<i32>(
            process,
            kAudioProcessPropertyPID,
            kAudioObjectPropertyScopeGlobal,
        ) else {
            continue;
        };
        if pid == std::process::id() as i32 {
            continue;
        }
        let path = process_path(pid);
        let owner = owners
            .iter()
            .find(|o| path.as_ref().is_some_and(|p| belongs_to(p, &o.0)))
            .or_else(|| owners.iter().find(|o| o.3 == pid));
        let (key, name) = if let Some(owner) = owner {
            (owner.1.clone(), owner.2.clone())
        } else {
            let bundle = audio::string(process, kAudioProcessPropertyBundleID).unwrap_or_default();
            if bundle.is_empty() {
                continue;
            }
            let name = path
                .as_ref()
                .and_then(|p| std::path::Path::new(p).file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| bundle.clone());
            (bundle, name)
        };
        let playing = audio::scalar::<u32>(
            process,
            kAudioProcessPropertyIsRunningOutput,
            kAudioObjectPropertyScopeGlobal,
        )
        .unwrap_or(0)
            != 0;
        let app = groups.entry(key.clone()).or_insert(App {
            key,
            name,
            processes: vec![],
            playing: false,
        });
        app.processes.push(process);
        app.playing |= playing;
    }
    let mut apps: Vec<_> = groups.into_values().collect();
    for app in &mut apps {
        app.processes.sort_unstable();
    }
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(apps)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_helpers_belong_to_the_outer_app_without_prefix_collisions() {
        assert!(belongs_to(
            "/Applications/Chrome.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper",
            "/Applications/Chrome.app"
        ));
        assert!(!belongs_to(
            "/Applications/Chrome.app.backup/Contents/MacOS/Chrome",
            "/Applications/Chrome.app"
        ));
        assert!(!belongs_to(
            "/Applications/Brave.app/Contents/MacOS/Brave",
            "/Applications/Chrome.app"
        ));
    }
}
