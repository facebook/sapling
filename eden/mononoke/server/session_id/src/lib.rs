/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use rand::distributions::Alphanumeric;
use rand::thread_rng;
use rand::Rng;
use std::fmt;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SessionId(String);

pub fn generate_session_id() -> SessionId {
    let s: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .map(char::from)
        .take(16)
        .collect();
    SessionId::from_string(s)
}

impl SessionId {
    pub fn from_string<T: ToString>(s: T) -> Self {
        Self(s.to_string())
    }

    pub fn to_string(&self) -> String {
        self.0.clone()
    }

    pub fn into_string(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Default for SessionId {
    fn default() -> Self {
        generate_session_id()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
