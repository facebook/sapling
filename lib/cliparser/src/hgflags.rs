// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::parser::{FlagDefinition, Value};

pub fn global_hg_flag_definitions() -> Vec<FlagDefinition> {
    let definitions = vec![
        (
            'R',
            "repository",
            "repository root directory or name of overlay bundle file",
            Value::Str("".to_string()),
        ),
        (
            ' ',
            "cwd",
            "change working directory",
            Value::Str("".to_string()),
        ),
        (
            'y',
            "noninteractive",
            "do not prompt, automatically pick the first choice for all prompts",
            Value::Bool(false),
        ),
        ('q', "quiet", "suppress output", Value::Bool(false)),
        (
            'v',
            "verbose",
            "enable additional output",
            Value::Bool(false),
        ),
        (
            ' ',
            "color",
            "when to colorize (boolean, always, auto, never, or debug)",
            Value::Str("".to_string()),
        ),
        (
            ' ',
            "config",
            "set/override config option (use 'section.name=value')",
            Value::List(Vec::new()),
        ),
        (
            ' ',
            "configfile",
            "enables the given config file",
            Value::List(Vec::new()),
        ),
        (' ', "debug", "enable debugging output", Value::Bool(false)),
        (' ', "debugger", "start debugger", Value::Bool(false)),
        (
            ' ',
            "encoding",
            "set the charset encoding",
            Value::Str("".to_string()),
        ),
        (
            ' ',
            "encodingmode",
            "set the charset encoding mode",
            Value::Str("strict".to_string()),
        ),
        (
            ' ',
            "traceback",
            "always print a traceback on exception",
            Value::Bool(false),
        ),
        (
            ' ',
            "time",
            "time how long the command takes",
            Value::Bool(false),
        ),
        (
            ' ',
            "profile",
            "print command execution profile",
            Value::Bool(false),
        ),
        (
            ' ',
            "version",
            "output version information and exit",
            Value::Bool(false),
        ),
        ('h', "help", "display help and exit", Value::Bool(false)),
        (
            ' ',
            "hidden",
            "consider hidden changesets",
            Value::Bool(false),
        ),
        (
            ' ',
            "pager",
            "when to paginate (boolean, always, auto, or never)",
            Value::Str("auto".to_string()),
        ),
    ];

    definitions
}
