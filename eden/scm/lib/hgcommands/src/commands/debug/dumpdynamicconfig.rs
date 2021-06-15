/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::define_flags;
use super::ConfigSet;
use super::Result;
use super::IO;
use configparser::dynamicconfig::Generator;

define_flags! {
    pub struct DebugDumpConfigOpts {
        /// repository name
        reponame: String,

        /// user name
        username: String,

        /// host name to fetch a canary config from
        canary: Option<String>,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(opts: DebugDumpConfigOpts, io: &IO, config: ConfigSet) -> Result<u8> {
    let reponame = opts.reponame;
    let mut username = opts.username;
    if username.is_empty() {
        username = config.get_opt("ui", "username")?.unwrap_or_default();
    }
    let canary = opts.canary;

    let temp_dir = std::env::temp_dir();
    let gen = Generator::new(reponame, temp_dir, username)?;
    let generated = gen.execute(canary)?;

    if opts.args.is_empty() {
        io.write(generated.to_string())?;
    } else {
        for arg in opts.args {
            let split: Vec<_> = arg.splitn(2, ".").collect();
            if let [section, name] = split[..] {
                let value: String = generated.get_opt(section, name)?.unwrap_or_default();
                io.write(format!("{}\n", value))?;
            }
        }
    }

    Ok(0)
}

pub fn name() -> &'static str {
    "debugdumpdynamicconfig"
}

pub fn doc() -> &'static str {
    "print the dynamic configuration

Without arguments, print the dynamic config in hgrc format.
Otherwise, print config values specified by the arguments.
An argument should be in the format ``section.name``.
"
}
