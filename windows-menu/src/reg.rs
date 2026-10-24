//! Thin safe wrapper over the Win32 registry API.

use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows_sys::Win32::System::Registry::*;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Registry hive an entry lives in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
    ClassesRoot,
}

impl Hive {
    pub fn root(self) -> HKEY {
        match self {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
            Hive::ClassesRoot => HKEY_CLASSES_ROOT,
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Hive::CurrentUser => "HKCU",
            Hive::LocalMachine => "HKLM",
            Hive::ClassesRoot => "HKCR",
        }
    }
    pub fn long(self) -> &'static str {
        match self {
            Hive::CurrentUser => "HKEY_CURRENT_USER",
            Hive::LocalMachine => "HKEY_LOCAL_MACHINE",
            Hive::ClassesRoot => "HKEY_CLASSES_ROOT",
        }
    }
}

pub struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    pub fn open(hive: Hive, path: &str) -> Option<Key> {
        open_raw(hive.root(), path)
    }

    pub fn sub(&self, name: &str) -> Option<Key> {
        open_raw(self.0, name)
    }

    pub fn subkeys(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut buf = [0u16; 512];
        for i in 0.. {
            let mut len = buf.len() as u32;
            let rc = unsafe {
                RegEnumKeyExW(self.0, i, buf.as_mut_ptr(), &mut len, null(), null_mut(), null_mut(), null_mut())
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        out
    }

    pub fn value_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut buf = [0u16; 512];
        for i in 0.. {
            let mut len = buf.len() as u32;
            let rc = unsafe {
                RegEnumValueW(self.0, i, buf.as_mut_ptr(), &mut len, null(), null_mut(), null_mut(), null_mut())
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        out
    }

    pub fn has_value(&self, name: &str) -> bool {
        let n = wide(name);
        unsafe { RegQueryValueExW(self.0, n.as_ptr(), null(), null_mut(), null_mut(), null_mut()) == ERROR_SUCCESS }
    }

    /// String value (REG_SZ / REG_EXPAND_SZ, unexpanded) plus its type. `""` = default value.
    pub fn get_str_typed(&self, name: &str) -> Option<(String, REG_VALUE_TYPE)> {
        let n = wide(name);
        let mut ty = 0;
        let mut size = 0u32;
        unsafe {
            if RegQueryValueExW(self.0, n.as_ptr(), null(), &mut ty, null_mut(), &mut size) != ERROR_SUCCESS {
                return None;
            }
            if ty != REG_SZ && ty != REG_EXPAND_SZ {
                return None;
            }
            let mut buf = vec![0u16; size as usize / 2 + 1];
            size = (buf.len() * 2) as u32;
            if RegQueryValueExW(self.0, n.as_ptr(), null(), &mut ty, buf.as_mut_ptr() as *mut u8, &mut size)
                != ERROR_SUCCESS
            {
                return None;
            }
            Some((from_wide(&buf), ty))
        }
    }

    pub fn get_str(&self, name: &str) -> Option<String> {
        self.get_str_typed(name).map(|(s, _)| s)
    }
}

fn open_raw(parent: HKEY, path: &str) -> Option<Key> {
    let p = wide(path);
    let mut h: HKEY = null_mut();
    let rc = unsafe { RegOpenKeyExW(parent, p.as_ptr(), 0, KEY_READ, &mut h) };
    (rc == ERROR_SUCCESS).then(|| Key(h))
}

/// Writes a string value, creating the key if needed.
pub fn set_str(hive: Hive, path: &str, name: &str, value: &str, ty: REG_VALUE_TYPE) -> Result<(), WIN32_ERROR> {
    let p = wide(path);
    let n = wide(name);
    let v = wide(value);
    unsafe {
        let mut h: HKEY = null_mut();
        let rc = RegCreateKeyExW(hive.root(), p.as_ptr(), 0, null(), 0, KEY_SET_VALUE, null(), &mut h, null_mut());
        if rc != ERROR_SUCCESS {
            return Err(rc);
        }
        let rc = RegSetValueExW(h, n.as_ptr(), 0, ty, v.as_ptr() as *const u8, (v.len() * 2) as u32);
        RegCloseKey(h);
        if rc != ERROR_SUCCESS {
            return Err(rc);
        }
    }
    Ok(())
}

/// Deletes a value; a missing key or value counts as success.
pub fn delete_value(hive: Hive, path: &str, name: &str) -> Result<(), WIN32_ERROR> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
    let p = wide(path);
    let n = wide(name);
    unsafe {
        let mut h: HKEY = null_mut();
        let rc = RegOpenKeyExW(hive.root(), p.as_ptr(), 0, KEY_SET_VALUE, &mut h);
        if rc == ERROR_FILE_NOT_FOUND || rc == ERROR_PATH_NOT_FOUND {
            return Ok(());
        }
        if rc != ERROR_SUCCESS {
            return Err(rc);
        }
        let rc = RegDeleteValueW(h, n.as_ptr());
        RegCloseKey(h);
        if rc != ERROR_SUCCESS && rc != ERROR_FILE_NOT_FOUND {
            return Err(rc);
        }
    }
    Ok(())
}
