/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use configmodel::Config;
use configmodel::convert::FromConfigValue;

mod effects;

pub use effects::ColorLevel;
pub use effects::Styler;
pub use effects::eval_style;

enum ColorMode {
    Off,
    Debug,
    Auto,
    Always,
}

impl ColorMode {
    pub fn from_config(val: &str, is_cli_flag: bool) -> Self {
        match val {
            "always" => Self::Always,
            "debug" => Self::Debug,
            "auto" => Self::Auto,

            // Otherwise, truthy CLI flag means "always", truthy config value means "auto".
            _ => match (bool::try_from_str(val).unwrap_or_default(), is_cli_flag) {
                (true, true) => Self::Always,
                (true, false) => Self::Auto,
                (false, _) => Self::Off,
            },
        }
    }
}

pub fn should_color(config: &dyn Config, file: &dyn io::Write) -> bool {
    if hgplain::is_plain(Some("color")) {
        return false;
    }

    let mode = ColorMode::from_config(
        &config.get("ui", "color").unwrap_or_default(),
        config
            .get_sources("ui", "color")
            .last()
            .is_some_and(|s| s.source().as_ref() == "--color"),
    );

    match mode {
        ColorMode::Off | ColorMode::Debug => false,
        ColorMode::Always => {
            // Call can_color() even if mode==always because can_color()
            // has side effects of configuring the terminal to better
            // support colors.
            let _ = file.can_color();
            true
        }
        ColorMode::Auto => file.can_color(),
    }
}
