/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::serde::Deserialize;
use ::serde::Serialize;
use termwiz::input::InputEvent;
use termwiz::input::KeyEvent;
use termwiz::input::MouseEvent;
use termwiz::input::PixelMouseEvent;

// Workaround of InputEvent didn't implement serde.
// PR: https://github.com/wezterm/wezterm/pull/7266
#[derive(Serialize, Deserialize)]
#[serde(rename = "InputEvent")]
pub enum InputEventSerde {
    Key(KeyEvent),
    Mouse(MouseEvent),
    PixelMouse(PixelMouseEvent),
    Resized { cols: usize, rows: usize },
    Paste(String),
    Wake,
}

impl From<InputEvent> for InputEventSerde {
    fn from(value: InputEvent) -> Self {
        match value {
            InputEvent::Key(v) => Self::Key(v),
            InputEvent::Mouse(v) => Self::Mouse(v),
            InputEvent::PixelMouse(v) => Self::PixelMouse(v),
            InputEvent::Resized { cols, rows } => Self::Resized { cols, rows },
            InputEvent::Paste(v) => Self::Paste(v),
            InputEvent::Wake => Self::Wake,
        }
    }
}
