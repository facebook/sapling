// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::errors;
use configparser::config::ConfigSet;
use failure::Fallible;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

pub struct Repo {
    path: PathBuf,
    config: ConfigSet,
    bundle_path: Option<PathBuf>,
}

impl Repo {
    pub fn new<P>(path: P, bundle_path: Option<PathBuf>) -> Self
    where
        P: Into<PathBuf>,
    {
        let path_buf: PathBuf = path.into();

        Repo {
            path: path_buf,
            config: ConfigSet::new(),
            bundle_path,
        }
    }

    pub fn sharedpath(&self) -> Fallible<Option<PathBuf>> {
        let mut sharedpath = fs::read_to_string(self.path.join(".hg/sharedpath"))
            .ok()
            .map(|s| PathBuf::from(s))
            .and_then(|p| Some(PathBuf::from(p.parent()?)));

        if let Some(possible_path) = sharedpath {
            if possible_path.is_absolute() && !possible_path.exists() {
                return Err(errors::InvalidSharedPath(
                    possible_path.join(".hg").to_string_lossy().to_string(),
                )
                .into());
            } else if possible_path.is_absolute() {
                sharedpath = Some(possible_path)
            } else {
                // join relative path from the REPO/.hg path
                let new_possible = self.path.join(".hg").join(possible_path);

                if !new_possible.join(".hg").exists() {
                    return Err(errors::InvalidSharedPath(
                        new_possible
                            .canonicalize()
                            .ok()
                            .map(|r| r.to_string_lossy().to_string())
                            .unwrap_or("".to_string()),
                    )
                    .into());
                }
                sharedpath = Some(new_possible)
            }
        }

        Ok(sharedpath)
    }

    pub fn set_config(&mut self, config: ConfigSet) {
        self.config = config
    }

    pub fn get_config(&self) -> &ConfigSet {
        &self.config
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}
