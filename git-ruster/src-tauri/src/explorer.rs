//! Windows Explorer context menu: "Open in Git Ruster" for folders and folder backgrounds.
//! Registered per user under HKCU\Software\Classes, so no admin rights are needed.

#[cfg(windows)]
mod imp {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const KEYS: [(&str, &str); 2] = [
        (r"Software\Classes\Directory\shell\GitRuster", "%1"),
        (r"Software\Classes\Directory\Background\shell\GitRuster", "%V"),
    ];

    fn exe() -> Result<String, String> {
        std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|e| e.to_string())
    }

    pub fn status() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        KEYS.iter().all(|(k, _)| hkcu.open_subkey(format!(r"{k}\command")).is_ok())
    }

    pub fn install() -> Result<(), String> {
        let exe = exe()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for (key, arg) in KEYS {
            let (k, _) = hkcu.create_subkey(key).map_err(|e| e.to_string())?;
            k.set_value("", &"Open in Git Ruster").map_err(|e| e.to_string())?;
            k.set_value("Icon", &format!("\"{exe}\",0")).map_err(|e| e.to_string())?;
            let (c, _) = k.create_subkey("command").map_err(|e| e.to_string())?;
            c.set_value("", &format!("\"{exe}\" \"{arg}\"")).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn uninstall() -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for (key, _) in KEYS {
            match hkcu.delete_subkey_all(key) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn status() -> bool {
        false
    }
    pub fn install() -> Result<(), String> {
        Err("Explorer integration is only available on Windows".into())
    }
    pub fn uninstall() -> Result<(), String> {
        Ok(())
    }
}

#[tauri::command]
pub fn context_menu_status() -> bool {
    imp::status()
}

#[tauri::command]
pub fn context_menu_install() -> Result<(), String> {
    imp::install()
}

#[tauri::command]
pub fn context_menu_uninstall() -> Result<(), String> {
    imp::uninstall()
}
