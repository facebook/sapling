use std;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

type StoredEnv = HashMap<OsString, OsString>;

/// Check if we are in the development environment
#[inline]
fn is_dev_env() -> bool {
    cfg!(feature = "hgdev")
}

/// A wrapper around environment variable-related functions
/// This functionality mainly serves to facilitate the use of `build/env` file
/// on Windows, which is populated by the preparation script.
pub struct HgEnv {
    stored: Option<StoredEnv>,
}

impl HgEnv {
    pub fn new() -> Self {
        let stored = if is_dev_env() {
            let exe_path = std::env::current_exe().unwrap();
            let installation_root = exe_path.parent().unwrap();
            Self::parse_stored_env(installation_root)
        } else {
            None
        };
        Self { stored }
    }

    /// Get the environment variable or the build environment config
    pub fn var_os<K: AsRef<OsStr>>(&self, key: K) -> Option<OsString> {
        let key = key.as_ref();
        self.stored
            .as_ref()
            .and_then(|hashmap| hashmap.get(key).cloned())
            .or_else(|| std::env::var_os(key))
    }

    /// Parse the stored environment file from ./build/env
    fn parse_stored_env<P: AsRef<Path>>(installation_root: P) -> Option<StoredEnv> {
        let build_env = "build/env";
        let env_path = installation_root.as_ref().join(build_env);
        std::fs::read_to_string(env_path)
            .and_then(|string| {
                string
                    .lines()
                    .map(|line| {
                        let split = line.splitn(2, '=').collect::<Vec<_>>();
                        if split.len() != 2 {
                            panic!("malformed env file at: '{}'", line);
                        }
                        Ok((OsString::from(split[0]), OsString::from(split[1])))
                    })
                    .collect()
            })
            .ok()
    }
}
