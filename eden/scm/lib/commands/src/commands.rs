/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

macro_rules! external_commands {
    [ $( $(#[$meta:meta])* $name:ident, )* ] => {
        pub(crate) fn extend_crate_command_table(table: &mut ::clidispatch::command::CommandTable) {
            $(
            $(#[$meta])*
            {
                use ::$name as m;
                let command_aliases = m::aliases();
                let doc = m::doc();
                let synopsis = m::synopsis();
                ::clidispatch::command::Register::register(table, m::run, &command_aliases, &doc, synopsis.as_deref());
            }
            )*
        }
    }
}

external_commands![
    // see update_commands.sh
    // [[[cog
    // import cog, glob, os, tomllib
    // with open('Cargo.toml', 'rb') as f:
    //     cargo = tomllib.load(f)
    // # Turn {'feature-foo': ['cmd-foo']} to {'cmd-foo': 'feature-foo'}
    // crate_to_feature = {}
    // for feature_name, feature_deps in cargo.get('features', {}).items():
    //     for feature_dep in feature_deps:
    //          if '/' not in feature_dep:
    //              crate_to_feature[feature_dep] = feature_name
    // for path in sorted(glob.glob('commands/cmd*/BUCK')) + sorted(glob.glob('debugcommands/cmd*/BUCK')):
    //     name = os.path.basename(os.path.dirname(path))
    //     feature_name = crate_to_feature.get(name)
    //     if feature_name:
    //          cog.outl(f'#[cfg(feature = "{feature_name}")]')
    //     cog.outl(f'{name},')
    // ]]]
    cmdcat,
    cmdclone,
    cmdconfig,
    cmdconfigfile,
    cmdgoto,
    cmdgrep,
    cmdroot,
    cmdstatus,
    cmdversion,
    cmdwhereami,
    #[cfg(feature = "eden")]
    cmdworktree,
    cmddebugargs,
    cmddebugconfigtree,
    cmddebugcurrentexe,
    cmddebugdumpindexedlog,
    cmddebugdumpinternalconfig,
    cmddebugfilterid,
    cmddebugfsync,
    cmddebuggitmodules,
    cmddebughash,
    cmddebughttp,
    cmddebuglfsreceive,
    cmddebuglfssend,
    cmddebugmergestate,
    cmddebugmetrics,
    cmddebugnetworkdoctor,
    cmddebugpython,
    cmddebugracyoutput,
    cmddebugrefreshconfig,
    cmddebugrevsets,
    cmddebugroots,
    cmddebugrunlog,
    cmddebugscmstore,
    cmddebugscmstorereplay,
    cmddebugsegmentgraph,
    cmddebugstore,
    cmddebugstructuredprogress,
    cmddebugtestcommand,
    cmddebugtop,
    cmddebugwait,
    cmddebugwalkdetector,
    // [[[end]]]
];

use clidispatch::command::CommandTable;

#[allow(dead_code)]
/// Return the main command table including all Rust commands.
pub fn table() -> CommandTable {
    let mut table = CommandTable::new();

    extend_crate_command_table(&mut table);

    table
}
