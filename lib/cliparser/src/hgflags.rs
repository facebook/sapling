// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::parser::{FlagDefinition, Value};

pub fn global_hg_flag_definitions() -> Vec<FlagDefinition> {
    let definitions = vec![
        (
            'R',
            "repository".into(),
            "repository root directory or name of overlay bundle file".into(),
            Value::Str("".to_string()),
        ),
        (
            ' ',
            "cwd".into(),
            "change working directory".into(),
            Value::Str("".to_string()),
        ),
        (
            ' ',
            "cwd".into(),
            "change working directory".into(),
            Value::Str("".to_string()),
        ),
        (
            'y',
            "noninteractive".into(),
            "do not prompt, automatically pick the first choice for all prompts".into(),
            Value::Bool(false),
        ),
        (
            'q',
            "quiet".into(),
            "suppress output".into(),
            Value::Bool(false),
        ),
        (
            'v',
            "verbose".into(),
            "enable additional output".into(),
            Value::Bool(false),
        ),
        (
            ' ',
            "color".into(),
            "when to colorize (boolean, always, auto, never, or debug)".into(),
            Value::Str("".to_string()),
        ),
        (
            ' ',
            "config".into(),
            "set/override config option (use 'section.name=value')".into(),
            Value::List(Vec::new()),
        ),
        (
            ' ',
            "configfile".into(),
            "enables the given config file".into(),
            Value::List(Vec::new()),
        ),
        (
            ' ',
            "debug".into(),
            "enable debugging output".into(),
            Value::Bool(false),
        ),
        (
            ' ',
            "debugger".into(),
            "start debugger".into(),
            Value::Bool(false),
        ),
        (
            ' ',
            "encoding".into(),
            "set the charset encoding".into(),
            Value::Str("".to_string()),
        ),
        (
            ' ',
            "encodingmode".into(),
            "set the charset encoding mode".into(),
            Value::Str("strict".to_string()),
        ),
        (
            ' ',
            "traceback".into(),
            "always print a traceback on exception".into(),
            Value::Bool(false),
        ),
        (
            ' ',
            "time".into(),
            "time how long the command takes".into(),
            Value::Bool(false),
        ),
        (
            ' ',
            "profile".into(),
            "print command execution profile".into(),
            Value::Bool(false),
        ),
        (
            ' ',
            "version".into(),
            "output version information and exit".into(),
            Value::Bool(false),
        ),
        (
            'h',
            "help".into(),
            "display help and exit".into(),
            Value::Bool(false),
        ),
        (
            ' ',
            "version".into(),
            "output version information and exit".into(),
            Value::Bool(false),
        ),
        (
            'h',
            "help".into(),
            "display help and exit".into(),
            Value::Bool(false),
        ),
        (
            ' ',
            "hidden".into(),
            "consider hidden changesets".into(),
            Value::Bool(false),
        ),
        (
            ' ',
            "pager".into(),
            "when to paginate (boolean, always, auto, or never)".into(),
            Value::Str("auto".to_string()),
        ),
    ];

    definitions
}
