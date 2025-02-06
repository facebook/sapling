/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[derive(Debug, thiserror::Error)]
pub enum Error {
    UnsupportedPatternKind(String),
    PathOutsideRoot(String, String, bool),
    NonUtf8(String),
    StdinUnavailable,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnsupportedPatternKind(s) => write!(f, "unsupported pattern kind {}", s),
            Error::PathOutsideRoot(path, root, show_hint) => {
                let message = format!("cwd relative path '{}' is not under root '{}'", path, root);
                if *show_hint {
                    let hint_message = "consider using --cwd to change working directory";
                    write!(f, "{}\n(hint: {})", message, hint_message)
                } else {
                    write!(f, "{}", message)
                }
            }
            Error::NonUtf8(s) => write!(f, "non-utf8 path '{}' when building pattern", s),
            Error::StdinUnavailable => write!(
                f,
                "listfile:- may only be used once as a direct CLI argument"
            ),
        }
    }
}
