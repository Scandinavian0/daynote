#[tauri::command]
pub async fn set_data_directory(path: String) -> Result<String, String> {
    daynote_core::paths::set_data_directory(std::path::Path::new(path.trim()))
        .map(|p| p.display().to_string())
}
