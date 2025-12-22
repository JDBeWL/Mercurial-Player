use tauri::{AppHandle, LogicalSize, Manager, Size};

#[tauri::command]
pub async fn set_mini_mode(app_handle: AppHandle, enable: bool) -> Result<(), String> {
    let window = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    if enable {
        // Enable Mini Mode
        let mini_size = LogicalSize {
            width: 320.0,
            height: 100.0,
        };

        // 1. Set both min and max size to the same value to prevent resizing
        window
            .set_min_size(Some(Size::Logical(mini_size)))
            .map_err(|e| e.to_string())?;
        
        window
            .set_max_size(Some(Size::Logical(mini_size)))
            .map_err(|e| e.to_string())?;

        // 2. Resize to mini size
        window
            .set_size(Size::Logical(mini_size))
            .map_err(|e| e.to_string())?;

        // 3. Disable resizable
        window.set_resizable(false).map_err(|e| e.to_string())?;

        // 4. Set always on top
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
    } else {
        // Disable Mini Mode

        // 1. Disable always on top
        window.set_always_on_top(false).map_err(|e| e.to_string())?;

        // 2. Enable resizable
        window.set_resizable(true).map_err(|e| e.to_string())?;

        // 3. Remove max size constraint
        window
            .set_max_size(None::<Size>)
            .map_err(|e| e.to_string())?;

        // 4. Restore default size (1250x720)
        window
            .set_size(Size::Logical(LogicalSize {
                width: 1250.0,
                height: 720.0,
            }))
            .map_err(|e| e.to_string())?;

        // 5. Restore min size constraints (1200x700)
        window
            .set_min_size(Some(Size::Logical(LogicalSize {
                width: 1200.0,
                height: 700.0,
            })))
            .map_err(|e| e.to_string())?;

        // 6. Center the window
        window.center().map_err(|e| e.to_string())?;
    }

    Ok(())
}
