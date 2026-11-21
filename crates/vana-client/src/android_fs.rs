//! Calls into `VanaClientNativeActivity` (Java) for storage access and the browser.

use jni::JNIEnv;
use jni::objects::JObject;

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

/// Opens the system Settings page for All files access.
pub fn request_all_files_access() {
    if let Err(e) = with_activity(|env, activity| {
        env.call_method(activity, "requestAllFilesAccess", "()V", &[])?;
        Ok(())
    }) {
        log::warn!("All files access settings: {e:#}");
    }
}

/// Folder picker file written by `FolderPickerProxyActivity`. `None` = still open.
pub fn consume_folder_pick() -> Option<Option<std::path::PathBuf>> {
    let dir = with_activity(|env, activity| {
        let file = env.call_method(activity, "getFilesDir", "()Ljava/io/File;", &[])?.l()?;
        let path = env.call_method(&file, "getAbsolutePath", "()Ljava/lang/String;", &[])?.l()?;
        let jstr = jni::objects::JString::from(path);
        Ok(env.get_string(&jstr)?.to_string_lossy().into_owned())
    })
    .inspect_err(|e| log::warn!("folder pick file: {e:#}"))
    .ok()?;
    let marker = std::path::Path::new(&dir).join("folder-pick.txt");
    if !marker.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&marker).unwrap_or_default();
    let _ = std::fs::remove_file(&marker);
    let text = text.trim();
    log::info!("folder pick file: '{text}'");
    if text.is_empty() { Some(None) } else { Some(Some(std::path::PathBuf::from(text))) }
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
