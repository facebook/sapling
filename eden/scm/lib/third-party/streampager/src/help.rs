//! Help screen

use std::fmt::Write;

use termwiz::input::{KeyCode, Modifiers};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::bindings::{Category, Keymap};
use crate::error::Result;

fn write_key_names(text: &mut String, keys: &[(Modifiers, KeyCode)]) -> Result<usize> {
    let mut w = 0;
    for (index, (modifiers, keycode)) in keys.iter().enumerate() {
        if index > 0 {
            if index == keys.len() - 1 {
                text.push_str("\x1B[0;2m or ");
                w += 4;
            } else {
                text.push_str("\x1B[0;2m, ");
                w += 2;
            }
        }
        text.push_str("\x1B[1m");
        for (modifier, desc) in [
            (Modifiers::CTRL, "Ctrl-"),
            (Modifiers::ALT, "Alt-"),
            (Modifiers::SUPER, "Super-"),
            (Modifiers::SHIFT, "Shift-"),
        ]
        .iter()
        {
            if modifiers.contains(*modifier) {
                text.push_str(desc);
                w += desc.width();
            }
        }
        match keycode {
            KeyCode::Char(' ') => {
                text.push_str("Space");
                w += 5;
            }
            KeyCode::Char(c) => {
                text.push(*c);
                w += c.width().unwrap_or(0);
            }
            KeyCode::Function(n) => {
                let n_string = n.to_string();
                text.push('F');
                text.push_str(&n_string);
                w += n_string.width() + 1;
            }
            KeyCode::UpArrow => {
                text.push_str("Up");
                w += 2;
            }
            KeyCode::DownArrow => {
                text.push_str("Down");
                w += 4;
            }
            KeyCode::LeftArrow => {
                text.push_str("Left");
                w += 4;
            }
            KeyCode::RightArrow => {
                text.push_str("Right");
                w += 5;
            }
            keycode => {
                let mut key_string = String::new();
                write!(key_string, "{:?}", keycode)?;
                text.push_str(&key_string);
                w += key_string.width();
            }
        }
    }
    text.push_str("\x1B[m");
    Ok(w)
}

pub(crate) fn help_text(keymap: &Keymap) -> Result<String> {
    let mut text = String::from(
        "\n  \x1B[1;3;36;38;5;39mStream Pager\x1B[m \x1B[35;38;57m(\x1B[1msp\x1B[22m)\n",
    );
    let prefix = "                                  ";

    for category in Category::categories() {
        let mut title = false;

        for (binding, keys) in keymap.iter_keys() {
            if binding.category() == category {
                if !title {
                    write!(text, "\n  \x1B[1;4;33;38;5;130m{}\x1B[m\n\n", category)?;
                    title = true;
                }
                text.push_str("    ");
                let w = write_key_names(&mut text, keys)?;
                if w < 34 {
                    text.push_str(&prefix[w..]);
                } else {
                    text.push_str("\n    ");
                    text.push_str(prefix);
                }
                writeln!(text, "{}", binding)?;
            }
        }
    }

    Ok(text)
}
