/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;

use crate::Generator;

pub(crate) fn fb_rules(gen: &mut Generator) -> Result<()> {
    gen.set_config("common", "hostgroup", gen.group().to_str());
    Ok(())
}
