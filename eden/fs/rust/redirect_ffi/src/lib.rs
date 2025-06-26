/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Result;
use anyhow::anyhow;
use edenfs_client::instance::EdenFsInstance;
use edenfs_client::redirect::Redirection;
use edenfs_client::redirect::RedirectionState;
use edenfs_client::redirect::RedirectionType;
use edenfs_client::redirect::get_effective_redirs_for_mount;
use edenfs_client::use_case::UseCaseId;

#[cxx::bridge]
mod ffi {

    // Original enum: RedirectionType
    // Mapping is ensured at compile-time by `match` in the From impl
    #[namespace = "facebook::eden"]
    #[repr(u32)]
    pub enum RedirectionType {
        BIND,
        SYMLINK,
        UNKNOWN,
    }

    // Original enum: RedirectionState
    // Mapping is ensured at compile-time by `match` in the From impl
    #[namespace = "facebook::eden"]
    #[repr(u32)]
    pub enum RedirectionState {
        MATCHES_CONFIGURATION,
        UNKNOWN_MOUNT,
        NOT_MOUNTED,
        SYMLINK_MISSING,
        SYMLINK_INCORRECT,
    }

    // Original struct: Redirection
    #[namespace = "facebook::eden"]
    pub struct RedirectionFFI {
        // Original type: PathBuf
        pub repo_path: String,
        // Original type: RedirectionType
        pub redir_type: RedirectionType,
        // Original type: PathBuf
        pub source: String,
        // Original type: RedirectionState
        pub state: RedirectionState,
        // Original type: Option<PathBuf>
        pub target: String,
    }

    #[namespace = "facebook::eden"]
    unsafe extern "C++" {
        // Declare enums to let cxx assert
        // they match thrift enums generated in C++
        include!("eden/fs/service/gen-cpp2/eden_types.h");
        type RedirectionType;
        type RedirectionState;
    }

    #[namespace = "facebook::eden"]
    extern "Rust" {

        fn list_redirections(
            mount: String,
            config_dir: String,
            etc_eden_dir: String,
        ) -> Result<Vec<RedirectionFFI>>;

    }
}

pub fn list_redirections(
    mount: String,
    config_dir: String,
    etc_eden_dir: String,
) -> Result<Vec<ffi::RedirectionFFI>, anyhow::Error> {
    // EdenFsInstance depends on having an initialized tokio runtime, but the
    // FFI layer does not guarantee this. As such, we create one here to
    // create and invoke methods on EdenFsInstance.
    use tokio::runtime;
    let rt = runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    // Execute code from within the new runtime
    let handle = rt.spawn_blocking(|| {
        let instance = EdenFsInstance::new(
            UseCaseId::RedirectFfi,
            config_dir.into(),
            etc_eden_dir.into(),
            None,
        );
        let redirs = get_effective_redirs_for_mount(&instance, mount.into())?;
        let redirs_ffi = redirs
            .values()
            .map(ffi::RedirectionFFI::try_from)
            .collect::<Result<Vec<_>>>()?;
        Ok(redirs_ffi)
    });
    // Block on future until it completes
    rt.block_on(handle)?
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

impl From<RedirectionType> for ffi::RedirectionType {
    fn from(redir_type: RedirectionType) -> Self {
        match redir_type {
            RedirectionType::Bind => ffi::RedirectionType::BIND,
            RedirectionType::Symlink => ffi::RedirectionType::SYMLINK,
            RedirectionType::Unknown => ffi::RedirectionType::UNKNOWN,
        }
    }
}

impl From<RedirectionState> for ffi::RedirectionState {
    fn from(redir_state: RedirectionState) -> Self {
        match redir_state {
            RedirectionState::MatchesConfiguration => ffi::RedirectionState::MATCHES_CONFIGURATION,
            RedirectionState::UnknownMount => ffi::RedirectionState::UNKNOWN_MOUNT,
            RedirectionState::NotMounted => ffi::RedirectionState::NOT_MOUNTED,
            RedirectionState::SymlinkMissing => ffi::RedirectionState::SYMLINK_MISSING,
            RedirectionState::SymlinkIncorrect => ffi::RedirectionState::SYMLINK_INCORRECT,
        }
    }
}

impl From<ffi::RedirectionType> for RedirectionType {
    fn from(redir: ffi::RedirectionType) -> Self {
        match redir {
            ffi::RedirectionType::BIND => RedirectionType::Bind,
            ffi::RedirectionType::SYMLINK => RedirectionType::Symlink,
            ffi::RedirectionType::UNKNOWN => RedirectionType::Unknown,
            // All the explicitly defined values are mapped above, but shared enums
            // in cxx::bridge need default handling for `match` to be exhaustive
            _ => RedirectionType::Unknown,
        }
    }
}

impl From<ffi::RedirectionState> for RedirectionState {
    fn from(redir_state: ffi::RedirectionState) -> RedirectionState {
        match redir_state {
            ffi::RedirectionState::MATCHES_CONFIGURATION => RedirectionState::MatchesConfiguration,
            ffi::RedirectionState::UNKNOWN_MOUNT => RedirectionState::UnknownMount,
            ffi::RedirectionState::NOT_MOUNTED => RedirectionState::NotMounted,
            ffi::RedirectionState::SYMLINK_MISSING => RedirectionState::SymlinkMissing,
            ffi::RedirectionState::SYMLINK_INCORRECT => RedirectionState::SymlinkIncorrect,
            // All the explicitly defined values are mapped above, but shared enums
            // in cxx::bridge need default handling for `match` to be exhaustive
            _ => RedirectionState::UnknownMount,
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
