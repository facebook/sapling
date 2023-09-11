/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use once_cell::sync::Lazy;

// compiled.rs is generated.
#[path = "compiled.rs"]
mod compiled;

pub use compiled::VERSION_MAJOR;
pub use compiled::VERSION_MINOR;

pub fn find_module(name: &str) -> Option<ModuleInfo> {
    compiled::MODULES.get(name).map(ModuleInfo)
}

pub fn list_modules() -> Vec<&'static str> {
    compiled::MODULES.keys().cloned().collect()
}

#[derive(Copy, Clone)]
pub struct ModuleInfo(&'static (&'static str, &'static [u8], bool, usize, usize));

impl ModuleInfo {
    pub fn c_name(&self) -> &'static [u8] {
        self.0.0.as_bytes()
    }

    pub fn name(&self) -> &'static str {
        &self.0.0[..self.0.0.len() - 1]
    }

    pub fn byte_code(&self) -> &'static [u8] {
        self.0.1
    }

    pub fn is_package(&self) -> bool {
        self.0.2
    }

    pub fn source_code(&self) -> &'static str {
        &UNCOMPRESS_SOURCE.as_str()[self.0.3..self.0.4]
    }
}

static UNCOMPRESS_SOURCE: Lazy<String> = Lazy::new(|| {
    let bytes = zstdelta::apply(b"", compiled::COMPRESSED_SOURCE).unwrap();
    String::from_utf8(bytes).unwrap()
});
