// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use cliparser::define_flags;

define_flags! {
    pub struct HgGlobalOpts {
        /// repository root directory or name of overlay bundle file
        #[short('R')]
        repository: String,

        /// change working directory
        cwd: String,

        /// do not prompt, automatically pick the first choice for all prompts
        #[short('y')]
        noninteractive: bool,

        /// suppress output
        #[short('q')]
        quiet: bool,

        /// enable additional output
        #[short('v')]
        verbose: bool,

        /// when to colorize (boolean, always, auto, never, or debug)
        color: String,

        /// set/override config option (use 'section.name=value')
        config: Vec<String>,

        /// enables the given config file
        configfile: Vec<String>,

        /// enable debugging output
        debug: bool,

        /// start debugger
        debugger: bool,

        /// set the charset encoding
        encoding: String,

        /// set the charset encoding mode
        encodingmode: String = "strict",

        /// always print a traceback on exception
        traceback: bool,

        /// time how long the command takes
        time: bool,

        /// print command execution profile
        profile: bool,

        /// output version information and exit
        version: bool,

        /// display help and exit
        #[short('h')]
        help: bool,

        /// consider hidden changesets
        hidden: bool,

        /// when to paginate (boolean, always, auto, or never)
        pager: String = "auto",
    }
}
