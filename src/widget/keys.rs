//! Key chords: the names a document writes in a `keydown:` map and the
//! chord a host's `KeyboardData` resolves to.
//!
//! A chord is modifiers plus one key, spelled `ctrl+shift+k` in a string
//! or `ctrl_shift_k` as a map key (a map literal's keys are identifiers,
//! and `+` is not one). Both joiners are accepted everywhere; the
//! normalised form is `+`-joined with the modifiers in a fixed order, so
//! `shift+ctrl+k` and `ctrl_shift_k` are the same listener.
//!
//! Key names are the ones the hosts already map to JS-style key codes
//! (`lorekeeper/app/src/lib.rs`): `escape`, `tab`, `enter`, `space`,
//! `backspace`, `delete`, `home`, `end`, the four `arrow*`s, letters,
//! digits and `f1`–`f12`. A code no name covers resolves to no chord at
//! all, so listeners are never consulted for it.

use super::event::KeyboardData;

/// Every key name a chord may end in, in the order the diagnostic lists
/// them. Letters, digits and F-keys are generated rather than listed.
pub const NAMED_KEYS: &[&str] = &[
    "escape",
    "tab",
    "enter",
    "space",
    "backspace",
    "delete",
    "home",
    "end",
    "arrowup",
    "arrowdown",
    "arrowleft",
    "arrowright",
];

/// The modifier names, in normalised order.
pub const MODIFIERS: &[&str] = &["ctrl", "alt", "shift", "meta"];

/// Backspace, Tab, Enter, Escape, Space, End, Home, arrows, Delete — the
/// JS `keyCode` values the hosts send.
const CODE_BACKSPACE: u32 = 8;
const CODE_TAB: u32 = 9;
const CODE_ENTER: u32 = 13;
const CODE_ESCAPE: u32 = 27;
const CODE_SPACE: u32 = 32;
const CODE_END: u32 = 35;
const CODE_HOME: u32 = 36;
const CODE_ARROW_LEFT: u32 = 37;
const CODE_ARROW_UP: u32 = 38;
const CODE_ARROW_RIGHT: u32 = 39;
const CODE_ARROW_DOWN: u32 = 40;
const CODE_DELETE: u32 = 46;
/// `F1` is 112 in the JS convention; `F12` is 123.
const CODE_F1: u32 = 112;
const CODE_F12: u32 = 123;

/// The key name for a host key code, or `None` for a code no chord can
/// name (modifier keys on their own, unmapped named keys sent as `0`).
///
/// Letters fold to lower case: a host that applies shift before
/// reporting sends `K` (75) for shift+k, and the chord already carries
/// the shift as a modifier.
pub fn key_name(code: u32) -> Option<String> {
    let name = match code {
        CODE_BACKSPACE => "backspace",
        CODE_TAB => "tab",
        CODE_ENTER => "enter",
        CODE_ESCAPE => "escape",
        CODE_SPACE => "space",
        CODE_END => "end",
        CODE_HOME => "home",
        CODE_ARROW_LEFT => "arrowleft",
        CODE_ARROW_UP => "arrowup",
        CODE_ARROW_RIGHT => "arrowright",
        CODE_ARROW_DOWN => "arrowdown",
        CODE_DELETE => "delete",
        CODE_F1..=CODE_F12 => return Some(format!("f{}", code - CODE_F1 + 1)),
        _ => {
            let ch = char::from_u32(code)?;
            if ch.is_ascii_alphabetic() {
                return Some(ch.to_ascii_lowercase().to_string());
            }
            if ch.is_ascii_digit() {
                return Some(ch.to_string());
            }
            return None;
        }
    };
    Some(name.to_string())
}

/// Whether a key press with this data is *text* — something a focused
/// text field consumes as a character — rather than a command key. A
/// chord with ctrl, alt or meta is never text; a bare letter, digit or
/// space (or any event carrying a `character`) is.
pub fn is_text_key(kb: &KeyboardData) -> bool {
    if kb.modifiers.ctrl || kb.modifiers.alt || kb.modifiers.meta {
        return false;
    }
    if kb.character.is_some() {
        return true;
    }
    match kb.key_code {
        Some(CODE_SPACE) => true,
        Some(code) => char::from_u32(code).is_some_and(|c| c.is_ascii_alphanumeric()),
        None => false,
    }
}

/// The normalised chord an event resolves to, or `None` when its key has
/// no name.
pub fn chord_of(kb: &KeyboardData) -> Option<String> {
    let key = key_name(kb.key_code?)?;
    Some(join(
        kb.modifiers.ctrl,
        kb.modifiers.alt,
        kb.modifiers.shift,
        kb.modifiers.meta,
        &key,
    ))
}

/// Parse an authored chord (`ctrl+k`, `ctrl_shift_z`, `escape`,
/// `digit1`) into its normalised form. `Err` names what was wrong, for
/// the builder to surface as a bridge error rather than a listener that
/// never fires.
pub fn normalize(spelling: &str) -> Result<String, String> {
    let lower = spelling.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return Err("a key chord cannot be empty".to_string());
    }
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut meta = false;
    let mut key: Option<String> = None;
    let parts: Vec<&str> = lower.split(['+', '_']).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err(format!("{spelling:?} names no key"));
    }
    let last = parts.len() - 1;
    for (i, part) in parts.iter().enumerate() {
        match *part {
            "ctrl" | "control" if i != last => ctrl = true,
            "alt" if i != last => alt = true,
            "shift" if i != last => shift = true,
            "meta" | "cmd" | "super" if i != last => meta = true,
            other if i == last => key = Some(canonical_key(other)?),
            other => {
                return Err(format!(
                    "{other:?} in {spelling:?} is not a modifier (ctrl, alt, shift, meta)"
                ))
            }
        }
    }
    let key = key.ok_or_else(|| format!("{spelling:?} names no key"))?;
    Ok(join(ctrl, alt, shift, meta, &key))
}

/// A key name in its canonical spelling, accepting the aliases a hand
/// reaches for (`esc`, `return`, `del`, `up`, `digit1`).
fn canonical_key(name: &str) -> Result<String, String> {
    let canonical = match name {
        "escape" | "esc" => "escape",
        "tab" => "tab",
        "enter" | "return" => "enter",
        "space" => "space",
        "backspace" => "backspace",
        "delete" | "del" => "delete",
        "home" => "home",
        "end" => "end",
        "arrowup" | "up" => "arrowup",
        "arrowdown" | "down" => "arrowdown",
        "arrowleft" | "left" => "arrowleft",
        "arrowright" | "right" => "arrowright",
        other => {
            let mut chars = other.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => return Ok(c.to_string()),
                _ => {}
            }
            if let Some(digit) = other.strip_prefix("digit") {
                if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) {
                    return Ok(digit.to_string());
                }
            }
            if let Some(n) = other.strip_prefix('f') {
                if let Ok(n) = n.parse::<u32>() {
                    if (1..=12).contains(&n) {
                        return Ok(format!("f{n}"));
                    }
                }
            }
            return Err(format!(
                "{name:?} is not a key name (known: {}, a-z, 0-9 or digit0-digit9, f1-f12)",
                NAMED_KEYS.join(", ")
            ));
        }
    };
    Ok(canonical.to_string())
}

fn join(ctrl: bool, alt: bool, shift: bool, meta: bool, key: &str) -> String {
    let mut out = String::new();
    for (on, name) in [
        (ctrl, "ctrl"),
        (alt, "alt"),
        (shift, "shift"),
        (meta, "meta"),
    ] {
        if on {
            out.push_str(name);
            out.push('+');
        }
    }
    out.push_str(key);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::event::KeyModifiers;

    fn data(code: u32, ctrl: bool, shift: bool) -> KeyboardData {
        KeyboardData {
            key_code: Some(code),
            character: None,
            modifiers: KeyModifiers {
                ctrl,
                alt: false,
                shift,
                meta: false,
            },
        }
    }

    #[test]
    fn both_joiners_normalise_to_one_spelling() {
        assert_eq!(normalize("ctrl+k").unwrap(), "ctrl+k");
        assert_eq!(normalize("ctrl_k").unwrap(), "ctrl+k");
        assert_eq!(normalize("shift+ctrl+K").unwrap(), "ctrl+shift+k");
        assert_eq!(normalize("Escape").unwrap(), "escape");
        assert_eq!(normalize("esc").unwrap(), "escape");
        assert_eq!(normalize("arrowdown").unwrap(), "arrowdown");
        assert_eq!(normalize("down").unwrap(), "arrowdown");
        assert_eq!(normalize("digit1").unwrap(), "1");
        assert_eq!(normalize("1").unwrap(), "1");
        assert_eq!(normalize("f5").unwrap(), "f5");
        assert_eq!(normalize("ctrl_shift_z").unwrap(), "ctrl+shift+z");
    }

    #[test]
    fn an_unknown_key_or_modifier_is_refused() {
        assert!(normalize("ctrl+").is_err());
        assert!(normalize("").is_err());
        assert!(normalize("hyper+k").is_err());
        assert!(normalize("f13").is_err());
        assert!(normalize("ctrl+kk").is_err());
        assert!(normalize("ctrl").is_err(), "a modifier alone names no key");
    }

    #[test]
    fn a_host_event_resolves_to_the_same_chord() {
        assert_eq!(
            chord_of(&data('k' as u32, true, false)).as_deref(),
            Some("ctrl+k")
        );
        // A host that shifts before reporting sends the capital.
        assert_eq!(
            chord_of(&data('K' as u32, false, true)).as_deref(),
            Some("shift+k")
        );
        assert_eq!(chord_of(&data(27, false, false)).as_deref(), Some("escape"));
        assert_eq!(
            chord_of(&data(40, false, false)).as_deref(),
            Some("arrowdown")
        );
        assert_eq!(chord_of(&data(114, false, false)).as_deref(), Some("f3"));
        assert_eq!(
            chord_of(&data(0, false, false)),
            None,
            "an unmapped key has no chord"
        );
    }

    #[test]
    fn text_keys_are_the_bare_printables() {
        assert!(is_text_key(&data('k' as u32, false, false)));
        assert!(is_text_key(&data(32, false, false)));
        assert!(!is_text_key(&data('k' as u32, true, false)));
        assert!(!is_text_key(&data(27, false, false)));
        assert!(!is_text_key(&data(40, false, false)));
    }
}
