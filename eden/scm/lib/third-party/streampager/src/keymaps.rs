//! Keymaps

use termwiz::input::{KeyCode, Modifiers};

use crate::bindings::{BindingConfig, Keymap};
use crate::keymap_error::{KeymapError, Result};

// Static data to generate a keymap.
type KeymapData = &'static [((Modifiers, KeyCode), BindingConfig)];

macro_rules! keymaps {
    ( $( $visibility:vis mod $name:ident ; )* ) => {
        $( $visibility mod $name ; )*

        pub(crate) static KEYMAPS: &'static [(&'static str, $crate::keymaps::KeymapData)] = &[
            $( ( stringify!( $name ), $crate::keymaps::$name::KEYMAP ), )*
        ];
    }
}

keymaps! {
    pub(crate) mod default;
}

pub(crate) fn load(name: &str) -> Result<Keymap> {
    for (keymap_name, keymap_data) in KEYMAPS {
        if &name == keymap_name {
            return Ok(Keymap::from(keymap_data.iter()));
        }
    }

    #[cfg(feature = "keymap-file")]
    {
        if let Some(mut path) = dirs::config_dir() {
            path.push("streampager");
            path.push("keymaps");
            path.push(name);
            if let Ok(keymap_data) = std::fs::read_to_string(&path) {
                let keymap_file = crate::keymap_file::KeymapFile::parse(&keymap_data)
                    .map_err(|err| err.with_file(path))?;
                return Ok(Keymap::from(keymap_file.iter()));
            }
        }
    }

    Err(KeymapError::MissingKeymap(name.to_string()))
}
