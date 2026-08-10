/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;

/// Command line arguments for controlling JustKnobs
#[derive(Args, Debug)]
pub struct JustKnobsArgs {
    /// Path to a config that contains JustKnob values. Enables
    /// cached_config-based JustKnobs implementation instead of the default one.
    #[clap(long)]
    pub just_knobs_config_path: Option<String>,

    /// Pin a single JustKnob, as `<knobset>:<name>=true|false|<int>`. May be
    /// repeated. Every other knob still resolves normally, unlike
    /// --just-knobs-config-path which replaces the knob source entirely.
    #[clap(long = "just-knob", value_name = "NAME=VALUE")]
    pub just_knob_overrides: Vec<String>,
}
