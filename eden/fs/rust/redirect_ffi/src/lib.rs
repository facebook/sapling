/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Result;
use edenfs_client::redirect::get_effective_redirs_for_mount;
use edenfs_client::redirect::Redirection;
use edenfs_client::redirect::RedirectionState;
use edenfs_client::redirect::RedirectionType;

#[cxx::bridge]
mod ffi {

    // Original enum: RedirectionType
    // Mapping is ensured at compile-time by `match` in the From impl
    pub enum RedirectionTypeFFI {
        Bind,
        Symlink,
        Unknown,
    }

    // Original enum: RedirectionState
    // Mapping is ensured at compile-time by `match` in the From impl
    pub enum RedirectionStateFFI {
        MatchesConfiguration,
        UnknownMount,
        NotMounted,
        SymlinkMissing,
        SymlinkIncorrect,
        None,
    }

    // Original struct: Redirection
    pub struct RedirectionFFI {
        // Original type: PathBuf
        pub repo_path: String,
        // Original type: RedirectionType
        pub redir_type: RedirectionTypeFFI,
        // Original type: PathBuf
        pub source: String,
        // Original type: Option<RedirectionState>
        pub state: RedirectionStateFFI,
        // Original type: Option<PathBuf>
        pub target: String,
    }

    #[namespace = "facebook::eden"]
    extern "Rust" {

        fn list_redirections(mount: String) -> Result<Vec<RedirectionFFI>>;

    }
}

pub fn list_redirections(mount: String) -> Result<Vec<ffi::RedirectionFFI>, anyhow::Error> {
    let redirs = get_effective_redirs_for_mount(mount.into())?;
    let redirs_ffi = redirs
        .values()
        .map(ffi::RedirectionFFI::try_from)
        .collect::<Result<Vec<_>>>()?;
    Ok(redirs_ffi)
}

// ============================================================================
// From & Into impls for FFI type conversions
// ============================================================================

impl TryFrom<&Redirection> for ffi::RedirectionFFI {
    type Error = anyhow::Error;

    fn try_from(redir: &Redirection) -> Result<Self, Self::Error> {
        let redir_ffi = ffi::RedirectionFFI {
            repo_path: pathbuf_to_string(redir.repo_path.clone())?,
            redir_type: redir.redir_type.into(),
            source: redir.source.to_owned(),
            state: redir.state.clone().into(),
            target: opt_pathbuf_to_string(&redir.target)?,
        };
        Ok(redir_ffi)
    }
}

impl From<RedirectionType> for ffi::RedirectionTypeFFI {
    fn from(redir_type: RedirectionType) -> Self {
        match redir_type {
            RedirectionType::Bind => ffi::RedirectionTypeFFI::Bind,
            RedirectionType::Symlink => ffi::RedirectionTypeFFI::Symlink,
            RedirectionType::Unknown => ffi::RedirectionTypeFFI::Unknown,
        }
    }
}

impl From<Option<RedirectionState>> for ffi::RedirectionStateFFI {
    fn from(redir_state: Option<RedirectionState>) -> Self {
        match redir_state {
            Some(provided) => match provided {
                RedirectionState::MatchesConfiguration => {
                    ffi::RedirectionStateFFI::MatchesConfiguration
                }
                RedirectionState::UnknownMount => ffi::RedirectionStateFFI::UnknownMount,
                RedirectionState::NotMounted => ffi::RedirectionStateFFI::NotMounted,
                RedirectionState::SymlinkMissing => ffi::RedirectionStateFFI::SymlinkMissing,
                RedirectionState::SymlinkIncorrect => ffi::RedirectionStateFFI::SymlinkIncorrect,
            },
            None => ffi::RedirectionStateFFI::None,
        }
    }
}

impl From<ffi::RedirectionTypeFFI> for RedirectionType {
    fn from(redir: ffi::RedirectionTypeFFI) -> Self {
        match redir {
            ffi::RedirectionTypeFFI::Bind => RedirectionType::Bind,
            ffi::RedirectionTypeFFI::Symlink => RedirectionType::Symlink,
            ffi::RedirectionTypeFFI::Unknown => RedirectionType::Unknown,
            // All the explicitly defined values are mapped above, but shared enums
            // in cxx::bridge need default handling for `match` to be exhaustive
            _ => RedirectionType::Unknown,
        }
    }
}

impl From<ffi::RedirectionStateFFI> for Option<RedirectionState> {
    fn from(redir_state: ffi::RedirectionStateFFI) -> Option<RedirectionState> {
        match redir_state {
            ffi::RedirectionStateFFI::MatchesConfiguration => {
                Some(RedirectionState::MatchesConfiguration)
            }
            ffi::RedirectionStateFFI::UnknownMount => Some(RedirectionState::UnknownMount),
            ffi::RedirectionStateFFI::NotMounted => Some(RedirectionState::NotMounted),
            ffi::RedirectionStateFFI::SymlinkMissing => Some(RedirectionState::SymlinkMissing),
            ffi::RedirectionStateFFI::SymlinkIncorrect => Some(RedirectionState::SymlinkIncorrect),
            ffi::RedirectionStateFFI::None => None,
            // All the explicitly defined values are mapped above, but shared enums
            // in cxx::bridge need default handling for `match` to be exhaustive
            _ => None,
        }
    }
}

impl TryInto<Redirection> for ffi::RedirectionFFI {
    type Error = anyhow::Error;

    fn try_into(self) -> std::result::Result<Redirection, Self::Error> {
        let redir = Redirection {
            repo_path: PathBuf::from(&self.repo_path),
            redir_type: self.redir_type.into(),
            source: self.source,
            state: self.state.into(),
            target: string_to_opt_pathbuf(self.target),
        };
        Ok(redir)
    }
}

// ============================================================================
// Private util functions for specific conversions due to FFI type limits
// ============================================================================

fn pathbuf_to_string(pb: PathBuf) -> Result<String> {
    let res = pb.into_os_string().into_string().map_err(|os_str| {
        anyhow!(
            "PathBuf can't be converted to String due to invalid UTF-8: {}",
            os_str.to_string_lossy()
        )
    })?;
    Ok(res)
}

fn opt_pathbuf_to_string(opt_pb: &Option<PathBuf>) -> Result<String> {
    let s = match opt_pb {
        Some(pb) => pathbuf_to_string(pb.into())?,
        None => "".into(),
    };
    Ok(s)
}

fn string_to_opt_pathbuf(s: String) -> Option<PathBuf> {
    if s.is_empty() { None } else { Some(s.into()) }
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(all(test, unix))]
mod test {
    use std::os::unix::ffi::OsStringExt;

    use super::*;

    #[test]
    fn test_pathbuf_string_conversion() {
        let path = "/foo/bar";
        let pathbuf = PathBuf::from(&path);
        assert_eq!(pathbuf_to_string(pathbuf).unwrap(), path);
        let invalid_path = std::ffi::OsString::from_vec(vec![0x80]);
        let pathbuf_with_invalid_path = PathBuf::from(&invalid_path);
        assert!(pathbuf_to_string(pathbuf_with_invalid_path).is_err());
    }

    #[test]
    fn test_opt_pathbuf_string_conversion() {
        let path = "/foo/bar";
        let opt_pb = Option::Some(PathBuf::from(&path));
        assert_eq!(opt_pathbuf_to_string(&opt_pb).unwrap(), path);
        assert_eq!(opt_pathbuf_to_string(&Option::None).unwrap(), "");
        assert_eq!(string_to_opt_pathbuf("".to_owned()), Option::None);
    }
}
