/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Per-process overrides for individual knobs.
//!
//! `--just-knobs-config-path` swaps out the knob source entirely, and the
//! replacement errors on any knob it doesn't contain, so overriding one knob
//! means enumerating every knob the process reads. These overrides are
//! consulted first and **fall through** on a miss, so a single knob can be
//! pinned while everything else resolves as it normally would.
//!
//! Intended for debugging and profiling, where the point is to run the
//! production code path with one knob flipped. Nothing changes unless
//! [`set_debug_overrides`] is called.

use std::collections::HashMap;
use std::sync::OnceLock;

use anyhow::Result;
use anyhow::anyhow;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KnobVal {
    Bool(bool),
    Int(i64),
}

/// Read on every knob evaluation. `OnceLock::get` is a single acquire load
/// while unset, which is what keeps that path cheap.
static OVERRIDES: OnceLock<HashMap<String, KnobVal>> = OnceLock::new();

/// Pin the given knobs for the lifetime of the process. Knobs that are absent
/// are unaffected and continue to resolve normally. Can only be called once.
pub fn set_debug_overrides(overrides: HashMap<String, KnobVal>) -> Result<()> {
    if overrides.is_empty() {
        return Ok(());
    }
    OVERRIDES
        .set(overrides)
        .map_err(|_| anyhow!("JustKnobs debug overrides have already been set"))
}

/// The knobs currently pinned, if any.
pub fn debug_overrides() -> Option<&'static HashMap<String, KnobVal>> {
    OVERRIDES.get()
}

/// `None` means no override applies and the caller should fall through.
pub(crate) fn eval(name: &str) -> Option<bool> {
    match debug_overrides()?.get(name)? {
        KnobVal::Bool(b) => Some(*b),
        KnobVal::Int(_) => panic!("JustKnob {name} is overridden with an int but read as a bool"),
    }
}

/// `None` means no override applies and the caller should fall through.
pub(crate) fn get(name: &str) -> Option<i64> {
    match debug_overrides()?.get(name)? {
        KnobVal::Int(i) => Some(*i),
        KnobVal::Bool(_) => panic!("JustKnob {name} is overridden with a bool but read as an int"),
    }
}

/// Parse `<knobset>:<name>=true|false|<int>`.
pub fn parse_override(arg: &str) -> Result<(String, KnobVal)> {
    let (name, value) = arg
        .split_once('=')
        .ok_or_else(|| anyhow!("Invalid JustKnob override {arg:?}, expected NAME=VALUE"))?;
    let value = match value {
        "true" => KnobVal::Bool(true),
        "false" => KnobVal::Bool(false),
        _ => KnobVal::Int(value.parse().map_err(|_| {
            anyhow!("Invalid JustKnob override value {value:?} for {name}, expected true|false|int")
        })?),
    };
    Ok((name.to_string(), value))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_overrides() -> Result<()> {
        assert_eq!(
            parse_override("scm/mononoke:foo=true")?,
            ("scm/mononoke:foo".to_string(), KnobVal::Bool(true))
        );
        assert_eq!(
            parse_override("scm/mononoke:foo=false")?,
            ("scm/mononoke:foo".to_string(), KnobVal::Bool(false))
        );
        assert_eq!(
            parse_override("scm/mononoke:bar=42")?,
            ("scm/mononoke:bar".to_string(), KnobVal::Int(42))
        );
        assert!(parse_override("scm/mononoke:foo").is_err());
        assert!(parse_override("scm/mononoke:foo=maybe").is_err());
        Ok(())
    }

    #[test]
    fn misses_fall_through() {
        assert_eq!(eval("scm/mononoke:never_set"), None);
        assert_eq!(get("scm/mononoke:never_set"), None);
    }
}
