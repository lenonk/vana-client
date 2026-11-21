//! Android's system folder picker (`VanaClientNativeActivity.requestFolderPicker`). The activity answers with a
//! plain path, read directly under All files access.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

static RESULT: Mutex<Option<Option<PathBuf>>> = Mutex::new(None);

/// Shared storage root. Listing this needs All files access; the system document picker is not involved.
pub fn storage_root() -> PathBuf {
    PathBuf::from("/storage/emulated/0")
}

/// A directory that exists, else its parent, else shared storage.
pub fn start_dir(hint: &Path) -> PathBuf {
    let mut p = if hint.as_os_str().is_empty() { storage_root() } else { hint.to_path_buf() };
    if !p.is_dir() {
        p = p.parent().map(Path::to_path_buf).unwrap_or_else(storage_root);
    }
    if !p.starts_with("/storage") { storage_root() } else { p }
}

pub enum BrowserAction {
    None,
    Up,
    Enter(PathBuf),
    Use,
    Cancel,
}

/// In-app folder list. `cwd` is shown; the caller applies [`BrowserAction`].
pub fn browser(ui: &mut egui::Ui, cwd: &Path) -> BrowserAction {
    let mut action = BrowserAction::None;
    ui.label(egui::RichText::new(cwd.display().to_string()).size(12.0).font(egui::FontId::monospace(12.0)));
    ui.horizontal(|ui| {
        if ui.button("Up").clicked() {
            action = BrowserAction::Up;
        }
        if ui.button("Use this folder").clicked() {
            action = BrowserAction::Use;
        }
        if ui.button("Cancel").clicked() {
            action = BrowserAction::Cancel;
        }
    });
    let mut kids: Vec<PathBuf> =
        std::fs::read_dir(cwd).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    kids.sort();
    let list_h = (ui.ctx().content_rect().height() * 0.45).clamp(140.0, 420.0);
    egui::ScrollArea::vertical().id_salt("folder list").max_height(list_h).auto_shrink([false; 2]).show(ui, |ui| {
        if kids.is_empty() {
            ui.label(egui::RichText::new("No subfolders (or this folder is not readable).").weak().size(12.0));
        }
        for dir in kids {
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if ui.button(format!("{name}/")).clicked() {
                action = BrowserAction::Enter(dir);
            }
        }
    });
    action
}

pub fn go_up(cwd: &Path) -> PathBuf {
    match cwd.parent() {
        Some(p) if p.starts_with("/storage") && p != Path::new("/") => p.to_path_buf(),
        _ => cwd.to_path_buf(),
    }
}

/// Clears any earlier answer and opens the picker near `start`.
pub fn start(start: &Path) {
    *RESULT.lock().unwrap() = None;
    let start = start.to_string_lossy().into_owned();
    // The picker is an activity: started from the Java main thread, never the event loop's (they deadlock).
    super::android_run_on_ui_thread(move || {
        let r = crate::android_fs::with_activity(|env, activity| {
            let hint = env.new_string(&start)?;
            env.call_method(activity, "requestFolderPicker", "(Ljava/lang/String;)V", &[(&hint).into()])?;
            Ok(())
        });
        if let Err(e) = r {
            log::warn!("folder picker: {e:#}");
            complete(None);
        }
    });
}

/// `Some(None)` when the user cancelled, `Some(Some(dir))` with a folder, `None` while the picker is open.
pub fn poll() -> Option<Option<PathBuf>> {
    RESULT.lock().unwrap().clone()
}

/// Takes the picker's answer once (JNI mutex, then a Java poll for the proxy picker).
pub fn take() -> Option<Option<PathBuf>> {
    if let Some(picked) = RESULT.lock().unwrap().take() {
        return Some(picked);
    }
    if let Some(picked) = crate::android_fs::consume_folder_pick() {
        return Some(picked);
    }
    None
}

fn complete(path: Option<PathBuf>) {
    if let Some(ref p) = path {
        log::info!("folder picker chose {}", p.display());
    } else {
        log::info!("folder picker cancelled");
    }
    *RESULT.lock().unwrap() = Some(path);
    super::android_resume_after_activity();
}

#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_vanaclient_VanaClientNativeActivity_onFolderPicked(
    mut env: jni::JNIEnv<'_>,
    _class: jni::objects::JClass<'_>,
    path: jni::objects::JString<'_>,
) {
    let path = env.get_string(&path).ok().map(|s| PathBuf::from(s.to_string_lossy().into_owned()));
    let _ = std::panic::catch_unwind(|| complete(path));
}

#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_vanaclient_VanaClientNativeActivity_onFolderCancelled(
    _env: jni::JNIEnv<'_>,
    _class: jni::objects::JClass<'_>,
) {
    let _ = std::panic::catch_unwind(|| complete(None));
}

#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_vanaclient_VanaClientNativeActivity_nativeAfterUiActivity(
    _env: jni::JNIEnv<'_>,
    _class: jni::objects::JClass<'_>,
) {
    let _ = std::panic::catch_unwind(super::android_resume_after_activity);
}
