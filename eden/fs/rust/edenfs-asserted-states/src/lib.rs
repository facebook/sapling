/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use edenfs_error::Result;

pub fn state_enter(_mount: &str, _state: &str) -> Result<()> {
    Ok(())
}

pub fn state_leave(_mount: &str, _state: &str) -> Result<()> {
    Ok(())
}

pub fn get_asserted_states(_mount: &str) -> Result<HashSet<String>> {
    Ok(HashSet::new())
}

pub fn is_state_asserted(_mount: &str, _state: &str) -> Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;

    use crate::get_asserted_states;

    #[fbinit::test]
    fn test_state_enter(_fb: FacebookInit) -> anyhow::Result<()> {
        let mount = "test_mount1";
        let asserted_states = get_asserted_states(mount)?;
        assert!(asserted_states.is_empty());
        Ok(())
    }
}
