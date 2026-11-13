//! Map API keys. On macOS they live in the user's Keychain; elsewhere (only
//! used for development) in a private file next to the settings.

use crate::tiles::KeyService;

#[cfg(target_os = "macos")]
const SERVICE: &str = "TimeAtlas map API key";

#[cfg(target_os = "macos")]
pub fn get(service: KeyService) -> Option<String> {
    keyring::Entry::new(SERVICE, service.id())
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.trim().is_empty())
}

#[cfg(target_os = "macos")]
pub fn set(service: KeyService, key: &str) -> Result<(), String> {
    keyring::Entry::new(SERVICE, service.id())
        .and_then(|e| e.set_password(key.trim()))
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
pub fn remove(service: KeyService) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, service.id()) {
        let _ = entry.delete_credential();
    }
}

#[cfg(not(target_os = "macos"))]
fn path() -> Option<std::path::PathBuf> {
    Some(dirs::config_dir()?.join("TimeAtlas").join("api-keys.json"))
}

#[cfg(not(target_os = "macos"))]
fn read_all() -> std::collections::HashMap<String, String> {
    path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
fn write_all(map: &std::collections::HashMap<String, String>) -> Result<(), String> {
    let path = path().ok_or("no config folder")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec(map).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn get(service: KeyService) -> Option<String> {
    read_all().remove(service.id()).filter(|k| !k.trim().is_empty())
}

#[cfg(not(target_os = "macos"))]
pub fn set(service: KeyService, key: &str) -> Result<(), String> {
    let mut all = read_all();
    all.insert(service.id().to_string(), key.trim().to_string());
    write_all(&all)
}

#[cfg(not(target_os = "macos"))]
pub fn remove(service: KeyService) {
    let mut all = read_all();
    all.remove(service.id());
    let _ = write_all(&all);
}
