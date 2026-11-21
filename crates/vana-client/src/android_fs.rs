//! Calls into `VanaClientNativeActivity` (Java) for storage access and the browser.

use jni::JNIEnv;
use jni::objects::JObject;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

enum AllFilesPoll {
    Unknown,
    Loading,
    Ready(bool),
}

static ALL_FILES: Mutex<AllFilesPoll> = Mutex::new(AllFilesPoll::Unknown);
static ALL_FILES_STALE: AtomicBool = AtomicBool::new(true);

/// Runs `f` with this thread attached to the JVM and the activity object.
pub fn with_activity<R>(f: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> anyhow::Result<R>) -> anyhow::Result<R> {
    let act = crate::client::android_activity_ptr().ok_or_else(|| anyhow::anyhow!("no Android activity"))?;
    let vm = ndk_context::android_context().vm();
    anyhow::ensure!(!vm.is_null(), "no JVM");
    // SAFETY: ndk_context holds the process's JavaVM and NativeActivity's global activity reference.
    let vm = unsafe { jni::JavaVM::from_raw(vm.cast())? };
    let mut env = vm.attach_current_thread()?;
    // The pointer is a process-wide global ref. Dropping JObject would DeleteLocalRef it.
    let activity = unsafe { JObject::from_raw(act.as_ptr().cast()) };
    let result = f(&mut env, &activity);
    std::mem::forget(activity);
    result
}

/// Android 11+'s All files access (`MANAGE_EXTERNAL_STORAGE`): the game files and installer volumes are read and
/// written as plain paths on shared storage.
pub fn has_all_files_access() -> bool {
    with_activity(|env, activity| Ok(env.call_method(activity, "hasAllFilesAccess", "()Z", &[])?.z()?)).unwrap_or(false)
}

/// Back from Settings or another activity: the next [`poll_all_files_access`] refresh runs off the UI thread.
pub fn stale_all_files_access() {
    ALL_FILES_STALE.store(true, Ordering::Release);
    let mut g = ALL_FILES.lock().unwrap();
    if !matches!(*g, AllFilesPoll::Loading) {
        *g = AllFilesPoll::Unknown;
    }
}

/// Last known All files access, if any. Kicks a JNI refresh on the stale flag (see [`stale_all_files_access`]).
pub fn poll_all_files_access() -> Option<bool> {
    if ALL_FILES_STALE.swap(false, Ordering::AcqRel) {
        let mut g = ALL_FILES.lock().unwrap();
        if !matches!(*g, AllFilesPoll::Loading) {
            *g = AllFilesPoll::Loading;
            std::thread::spawn(|| {
                let ok = has_all_files_access();
                *ALL_FILES.lock().unwrap() = AllFilesPoll::Ready(ok);
            });
        }
    }
    match &*ALL_FILES.lock().unwrap() {
        AllFilesPoll::Ready(ok) => Some(*ok),
        _ => None,
    }
}

/// Opens the system Settings page for All files access.
pub fn request_all_files_access() {
    if let Err(e) = with_activity(|env, activity| {
        env.call_method(activity, "requestAllFilesAccess", "()V", &[])?;
        Ok(())
    }) {
        log::warn!("All files access settings: {e:#}");
    }
    stale_all_files_access();
}

/// Opens `url` in the system browser.
pub fn open_url(url: &str) {
    if let Err(e) = with_activity(|env, activity| {
        let url = env.new_string(url)?;
        env.call_method(activity, "openBrowser", "(Ljava/lang/String;)V", &[(&url).into()])?;
        Ok(())
    }) {
        log::warn!("opening {url}: {e:#}");
    }
}
