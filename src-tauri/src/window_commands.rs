use tauri::{AppHandle, LogicalSize, Manager, Size};

#[tauri::command]
pub async fn set_mini_mode(app_handle: AppHandle, enable: bool) -> Result<(), String> {
    let window = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    if enable {
        // Enable Mini Mode

        // 1. Remove min size constraints
        // Setting min size to 0 allows shrinking
        window
            .set_min_size(Some(Size::Logical(LogicalSize {
                width: 0.0,
                height: 0.0,
            })))
            .map_err(|e| e.to_string())?;

        // 2. Resize to mini size (300x115 seems like a good compact size for controls + art)
        // Adjust based on UI design later if needed
        window
            .set_size(Size::Logical(LogicalSize {
                width: 300.0,
                height: 115.0,
            }))
            .map_err(|e| e.to_string())?;

        // 3. Set always on top
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
    } else {
        // Disable Mini Mode

        // 1. Disable always on top
        window.set_always_on_top(false).map_err(|e| e.to_string())?;

        // 2. Restore default size (1250x720)
        window
            .set_size(Size::Logical(LogicalSize {
                width: 1250.0,
                height: 720.0,
            }))
            .map_err(|e| e.to_string())?;

        // 3. Restore min size constraints (1200x700)
        window
            .set_min_size(Some(Size::Logical(LogicalSize {
                width: 1200.0,
                height: 700.0,
            })))
            .map_err(|e| e.to_string())?;

        // 4. Center the window
        window.center().map_err(|e| e.to_string())?;
    }

    Ok(())
}
