//! Parsing and matching of key combinations such as `"Ctrl+Shift+S"`.
//!
//! A binding string may hold several alternatives separated by `;`, e.g.
//! `"Esc;Enter;Ctrl+C"`.

use std::fmt;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KeyCombo {
    pub vk: u16,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl KeyCombo {
    /// The combo currently being pressed, given the virtual key from a
    /// WM_KEYDOWN / WM_SYSKEYDOWN message.
    pub fn from_keydown(vk: u16) -> Self {
        let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k as i32) } < 0;
        KeyCombo {
            vk,
            ctrl: down(VK_CONTROL),
            alt: down(VK_MENU),
            shift: down(VK_SHIFT),
            win: down(VK_LWIN) || down(VK_RWIN),
        }
    }

    /// Modifier flags for `RegisterHotKey`.
    pub fn hotkey_mods(&self) -> HOT_KEY_MODIFIERS {
        let mut m = MOD_NOREPEAT;
        if self.ctrl {
            m |= MOD_CONTROL;
        }
        if self.alt {
            m |= MOD_ALT;
        }
        if self.shift {
            m |= MOD_SHIFT;
        }
        if self.win {
            m |= MOD_WIN;
        }
        m
    }
}

const NAMED: &[(&str, VIRTUAL_KEY)] = &[
    ("esc", VK_ESCAPE),
    ("escape", VK_ESCAPE),
    ("enter", VK_RETURN),
    ("return", VK_RETURN),
    ("space", VK_SPACE),
    ("tab", VK_TAB),
    ("backspace", VK_BACK),
    ("delete", VK_DELETE),
    ("del", VK_DELETE),
    ("insert", VK_INSERT),
    ("ins", VK_INSERT),
    ("home", VK_HOME),
    ("end", VK_END),
    ("pageup", VK_PRIOR),
    ("pgup", VK_PRIOR),
    ("pagedown", VK_NEXT),
    ("pgdn", VK_NEXT),
    ("up", VK_UP),
    ("down", VK_DOWN),
    ("left", VK_LEFT),
    ("right", VK_RIGHT),
    ("printscreen", VK_SNAPSHOT),
    ("prtsc", VK_SNAPSHOT),
    ("prtscr", VK_SNAPSHOT),
    ("print", VK_SNAPSHOT),
    ("pause", VK_PAUSE),
    ("scrolllock", VK_SCROLL),
    ("plus", VK_OEM_PLUS),
    ("minus", VK_OEM_MINUS),
    ("comma", VK_OEM_COMMA),
    ("period", VK_OEM_PERIOD),
    ("numpad0", VK_NUMPAD0),
    ("numpad1", VK_NUMPAD1),
    ("numpad2", VK_NUMPAD2),
    ("numpad3", VK_NUMPAD3),
    ("numpad4", VK_NUMPAD4),
    ("numpad5", VK_NUMPAD5),
    ("numpad6", VK_NUMPAD6),
    ("numpad7", VK_NUMPAD7),
    ("numpad8", VK_NUMPAD8),
    ("numpad9", VK_NUMPAD9),
];

fn parse_key(name: &str) -> Option<u16> {
    let lower = name.to_ascii_lowercase();
    if let Some(&(_, vk)) = NAMED.iter().find(|(n, _)| *n == lower) {
        return Some(vk);
    }
    let b = lower.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_alphanumeric()) {
        return Some(b[0].to_ascii_uppercase() as u16); // VK_A..VK_Z / VK_0..VK_9
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
        if (1..=24).contains(&n) {
            return Some(VK_F1 + n - 1);
        }
    }
    None
}

impl std::str::FromStr for KeyCombo {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let mut c = KeyCombo::default();
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        // "Ctrl++" style is not supported; use "Ctrl+Plus".
        for (i, p) in parts.iter().enumerate() {
            let last = i == parts.len() - 1;
            match p.to_ascii_lowercase().as_str() {
                "ctrl" | "control" if !last => c.ctrl = true,
                "alt" if !last => c.alt = true,
                "shift" if !last => c.shift = true,
                "win" | "super" | "meta" if !last => c.win = true,
                _ if last => {
                    c.vk = parse_key(p).ok_or_else(|| format!("unknown key '{p}' in '{s}'"))?;
                }
                _ => return Err(format!("unknown modifier '{p}' in '{s}'")),
            }
        }
        if c.vk == 0 {
            return Err(format!("no key in '{s}'"));
        }
        Ok(c)
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        if self.win {
            f.write_str("Win+")?;
        }
        let vk = self.vk;
        match vk {
            0x30..=0x39 | 0x41..=0x5A => write!(f, "{}", vk as u8 as char),
            _ if (VK_F1..=VK_F24).contains(&vk) => write!(f, "F{}", vk - VK_F1 + 1),
            _ => {
                let name = match NAMED.iter().find(|(_, v)| *v == vk) {
                    Some(("esc", _)) => "Esc",
                    Some(("enter", _)) => "Enter",
                    Some(("printscreen", _)) => "PrintScreen",
                    Some((n, _)) => n,
                    None => "?",
                };
                let mut chars = name.chars();
                if let Some(first) = chars.next() {
                    write!(f, "{}{}", first.to_ascii_uppercase(), chars.as_str())?;
                }
                Ok(())
            }
        }
    }
}

/// A list of alternative combos, parsed from `"Esc;Enter;Ctrl+C"`.
#[derive(Clone, Debug, Default)]
pub struct Binding(pub Vec<KeyCombo>);

impl Binding {
    /// Parse a binding; invalid parts are reported in `errors` and skipped.
    pub fn parse(s: &str, errors: &mut Vec<String>) -> Self {
        let mut v = Vec::new();
        for part in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            match part.parse() {
                Ok(c) => v.push(c),
                Err(e) => errors.push(e),
            }
        }
        Binding(v)
    }

    pub fn matches(&self, c: &KeyCombo) -> bool {
        self.0.iter().any(|b| b == c)
    }

    /// Human readable, e.g. "Esc / Enter / Ctrl+C".
    pub fn label(&self) -> String {
        self.0.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" / ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combos() {
        let mut e = vec![];
        let b = Binding::parse("Esc;Enter;Ctrl+C", &mut e);
        assert!(e.is_empty());
        assert_eq!(b.0.len(), 3);
        assert_eq!(b.0[0].vk, VK_ESCAPE);
        assert_eq!(b.0[2], KeyCombo { vk: b'C' as u16, ctrl: true, ..Default::default() });
        assert_eq!(b.label(), "Esc / Enter / Ctrl+C");
        let f10: KeyCombo = "F10".parse().unwrap();
        assert_eq!(f10.vk, VK_F10);
        assert_eq!("ctrl+shift+s".parse::<KeyCombo>().unwrap().to_string(), "Ctrl+Shift+S");
        assert_eq!("PrintScreen".parse::<KeyCombo>().unwrap().to_string(), "PrintScreen");
        assert!("Ctrl+Nope".parse::<KeyCombo>().is_err());
        assert!("Ctrl".parse::<KeyCombo>().is_err());
    }
}
