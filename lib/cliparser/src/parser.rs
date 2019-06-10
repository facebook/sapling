// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use failure::{bail, Fallible};
use std::collections::HashMap;
use std::convert::TryInto;

/// FlagDefinition represents a tuple of options that represent
/// a single definition of a flag configured by each property.
///
/// | Type         | Meaning |
/// | ---          | --- |
/// | char         | short_name of a flag i.e. '-q' |
/// | &str         | long_name of a flag i.e. '--quiet' |
/// | &str         | description of a flag i.e. 'silences the output' |
/// | Value        | The expected type of value as well as a default |
///
/// To omit a short_name, pass in empty character ' '
///
/// To omit a long_name, pass in a blank string or a string with just whitespace
///
///
/// ```
/// use cliparser::parser::{Value, FlagDefinition};
///
/// let def: FlagDefinition = ('q',
///     "quiet",
///     "silences the output",
///     Value::Bool(false));
/// ```
pub type FlagDefinition = (char, &'static str, &'static str, Value);

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Value {
    Bool(bool),
    Str(String),
    Int(i64),
    List(Vec<String>),
}

impl Value {
    fn accept(&mut self, token_opt: Option<&str>) -> Fallible<()> {
        let token = match token_opt {
            Some(s) => s,
            None => return Ok(()),
        };

        match self {
            Value::Bool(_) => bail!("Bool doesn't accept a token to parse!"),
            Value::Str(ref mut s) => {
                *s = token.to_string();
                Ok(())
            }
            Value::Int(ref mut i) => {
                *i = token.parse::<i64>()?;
                Ok(())
            }
            Value::List(ref mut vec) => {
                vec.push(token.to_string());
                Ok(())
            }
        }
    }
}

impl TryInto<String> for Value {
    type Error = failure::Error;

    fn try_into(self) -> Fallible<String> {
        match self {
            Value::Str(s) => Ok(s),
            _ => bail!("Only Value::Str can convert to String!"),
        }
    }
}

impl TryInto<i64> for Value {
    type Error = failure::Error;

    fn try_into(self) -> Fallible<i64> {
        match self {
            Value::Int(i) => Ok(i),
            _ => bail!("Only Value::Int can convert to i64!"),
        }
    }
}

impl TryInto<bool> for Value {
    type Error = failure::Error;

    fn try_into(self) -> Fallible<bool> {
        match self {
            Value::Bool(b) => Ok(b),
            _ => bail!("Only Value::Bool can convert to bool!"),
        }
    }
}

impl TryInto<Vec<String>> for Value {
    type Error = failure::Error;

    fn try_into(self) -> Fallible<Vec<String>> {
        match self {
            Value::List(vec) => Ok(vec),
            _ => bail!("Only Value::List can convert to List!"),
        }
    }
}

/// Flag holds information about a configurable flag to be used during parsing CLI args.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Flag<'a> {
    /// short_name of a flag i.e. `q`
    short_name: Option<char>,
    /// long_name of a flag i.e. `quiet`
    long_name: &'a str,
    /// description of a flag i.e. `silences the output`
    description: Option<&'a str>,
    value_type: &'a Value,
}

impl<'a> Flag<'a> {
    /// Create a new Flag struct from a given FlagDefinition.
    ///
    /// ```
    /// use cliparser::parser::*;
    /// let def = ('q', "quiet", "silences the output", Value::Bool(false));
    /// let flag = Flag::new(&def);
    /// ```
    ///
    /// If no short_name should be used, provide an empty char ' '
    /// ```
    /// use cliparser::parser::*;
    /// let def = (' ', "quiet", "silences the output", Value::Bool(false));
    /// ```
    ///
    /// If no description should be used, provide an empty string
    /// ```
    /// use cliparser::parser::*;
    /// let def = ('q', "quiet", "", Value::Bool(false));
    /// ```
    ///
    pub fn new(definition: &'a FlagDefinition) -> Self {
        let short_name_opt = match definition.0 {
            ' ' => None,
            _ => Some(definition.0),
        };

        let long_name = definition.1;

        let description_opt = match definition.2 {
            description if description.is_empty() => None,
            _ => Some(definition.2),
        };

        let value_type = &definition.3;

        Flag {
            short_name: short_name_opt,
            long_name,
            description: description_opt,
            value_type,
        }
    }

    /// Create a vector of Flags from a collection of FlagDefinition.
    ///
    /// ```
    /// use cliparser::parser::*;
    ///
    /// let defs: Vec<FlagDefinition> = vec![
    /// ('q', "quiet", "silences the output", Value::Bool(false)),
    /// ('c', "config", "supply config file", Value::Str("".to_string())),
    /// ('h', "help", "get some help", Value::Bool(false)),
    /// ('v', "verbose", "level of verbosity", Value::Bool(false)),
    /// ];
    ///
    /// let flags = Flag::from_flags(&defs);
    /// assert_eq!(flags.len(), 4);
    /// ```
    pub fn from_flags(definitions: &'a [FlagDefinition]) -> Vec<Flag<'a>> {
        definitions.iter().map(|def| Flag::new(def)).collect()
    }
}

/// [`Parser`] keeps flag definitions and uses them to parse string arguments.
pub struct Parser<'a> {
    /// map holding &character -> &flag where the character == flag.short_name
    short_map: HashMap<&'a char, &'a Flag<'a>>,
    /// map holding &str -> &flag where the str == flag.long_name
    long_map: HashMap<&'a str, &'a Flag<'a>>,
    opts: HashMap<String, Value>,
}

impl<'a> Parser<'a> {
    /// initialize and setup a parser with all known flag definitions
    /// ```
    /// use cliparser::parser::*;
    ///
    /// let definitions = vec![
    /// ('c', "config", "supply a config", Value::Bool(false)),
    /// ('h', "help", "get some help", Value::Bool(false)),
    /// ('q', "quiet", "silence the output", Value::Bool(false))
    /// ];
    ///
    /// let flags = Flag::from_flags(&definitions);
    /// let parser = Parser::new(&flags);
    /// ```
    pub fn new(flags: &'a Vec<Flag<'a>>) -> Self {
        let mut short_map = HashMap::new();
        let mut long_map = HashMap::new();
        let mut opts = HashMap::new();

        for flag in flags {
            if let Some(ref character) = flag.short_name {
                short_map.insert(character, flag);
            }

            long_map.insert(flag.long_name, flag);
            let value_opt = match flag.value_type {
                _ => flag.value_type.clone(),
            };
            opts.insert(flag.long_name.to_string(), value_opt);
        }

        Parser {
            short_map,
            long_map,
            opts,
        }
    }

    /// entry-point for parsing command line arguments from std::env
    ///
    /// ```
    /// use std::env;
    /// use cliparser::parser::*;
    ///
    /// let env_args = env::args().collect();
    ///
    /// let definitions = vec![
    /// ('q', "quiet", "silence the output", Value::Bool(false))
    /// ];
    ///
    /// let flags = Flag::from_flags(&definitions);
    /// let parser = Parser::new(&flags);
    ///
    /// parser.parse_args(&env_args);
    /// ```
    ///
    /// parse_args will clean arguments such that they can be properly parsed by Parser#_parse
    pub fn parse_args(&self, args: &'a Vec<String>) -> Fallible<ParseOutput> {
        let arg_vec: Vec<&'a str> = args
            .iter()
            .map(|string| &string[..])
            .collect();

        self._parse(arg_vec)
    }

    pub fn _parse(&self, args: Vec<&'a str>) -> Fallible<ParseOutput> {
        let mut opts = self.opts.clone();
        let mut iter = args.into_iter().peekable();
        let mut positional_args = Vec::new();
        let mut unknown_args = Vec::new();

        while let Some(&arg) = iter.peek() {
            if arg.eq("--") {
                let _ = iter.next(); // don't care about -- it's just a separator
                positional_args.extend(iter);
                break;
            } else if arg.starts_with("--") {
                if let Err(_msg) = self.parse_double_hyphen_flag(&mut iter, &mut opts) {
                    // TODO implement actual error handling with Fallible
                    unknown_args.push(arg);
                }
            } else if arg.starts_with("-") {
                if let Err(_msg) = self.parse_single_hyphen_flag(&mut iter, &mut opts) {
                    // TODO implement actual error handling with Fallible
                    unknown_args.push(arg);
                }
            } else {
                positional_args.push(arg);
                iter.next();
            }
        }

        Ok(ParseOutput::new(
            opts,
            positional_args.iter().map(|s| s.to_string()).collect(),
        ))
    }

    fn parse_double_hyphen_flag(
        &self,
        iter: &mut Iterator<Item = &'a str>,
        opts: &mut HashMap<String, Value>,
    ) -> Fallible<()> {
        let arg = iter.next().unwrap();
        debug_assert!(arg.starts_with("--"));
        let arg = &arg[2..];
        let (arg, positive_flag) = if arg.starts_with("no-") {
            (&arg[3..], false)
        } else {
            (arg, true)
        };

        let mut parts = arg.splitn(2, "=");
        let clean_arg = parts.next().unwrap();
        let next = parts.next().or_else(|| iter.next());

        if let Some(known_flag) = self.long_map.get(clean_arg) {
            match opts.get_mut(known_flag.long_name) {
                Some(Value::Bool(ref mut b)) => *b = positive_flag,
                Some(ref mut value) => {
                    value.accept(next)?;
                }
                None => unreachable!(),
            }
            return Ok(());
        };

        bail!("Could not parse a flag!")
    }

    fn parse_single_hyphen_flag(
        &self,
        iter: &mut Iterator<Item = &'a str>,
        opts: &mut HashMap<String, Value>,
    ) -> Fallible<()> {
        let clean_arg = iter.next().unwrap().trim_start_matches("-");

        let mut char_iter = clean_arg.chars().peekable();

        while let Some(curr_char) = char_iter.next() {
            if let Some(known_flag) = self.short_map.get(&curr_char) {
                let flag_name = known_flag.long_name.to_string();
                match opts.get_mut(&flag_name) {
                    Some(Value::Bool(ref mut b)) => *b = true,
                    Some(ref mut value) => {
                        if char_iter.peek().is_none() {
                            let next = iter.next();
                            value.accept(next)?;
                        } else {
                            let consumed = char_iter.collect::<String>();
                            let consumed = Some(&consumed[..]);
                            value.accept(consumed)?;
                            break;
                        }
                    }
                    None => unreachable!(),
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseOutput {
    /// The opts
    opts: HashMap<String, Value>,
    /// The positional args
    args: Vec<String>,
}

/// ParseOutput represents all of the information successfully parsed from the command-line
/// arguments, as well as exposing a convenient API for application logic to query results
/// parsed.
impl ParseOutput {
    pub fn new(opts: HashMap<String, Value>, args: Vec<String>) -> Self {
        ParseOutput { opts, args }
    }

    pub fn get(&self, long_name: &str) -> Option<&Value> {
        self.opts.get(long_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definitions() -> Vec<FlagDefinition> {
        vec![
            ('q', "quiet", "silences the output", Value::Bool(false)),
            ('c', "config", "supply config file", Value::List(Vec::new())),
            ('h', "help", "get some help", Value::Bool(false)),
            ('v', "verbose", "level of verbosity", Value::Bool(false)),
            ('r', "rev", "revision hash", Value::Str("".to_string())),
        ]
    }

    fn create_args(strings: Vec<&str>) -> Vec<String> {
        strings.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_create_1_flag() {
        let def = ('q', "quiet", "silences the output", Value::Bool(false));
        let flag = Flag::new(&def);
        assert_eq!('q', flag.short_name.unwrap());
        assert_eq!("quiet", flag.long_name);
        assert_eq!("silences the output", flag.description.unwrap());
        assert_eq!(&Value::Bool(false), flag.value_type);
    }

    #[test]
    fn test_create_1_flag_with_empty_short_name() {
        let def = (' ', "quiet", "silences the output", Value::Bool(false));
        let flag = Flag::new(&def);
        assert!(flag.short_name.is_none());
    }

    #[test]
    fn test_create_1_flag_with_empty_description() {
        let def = ('q', "quiet", "", Value::Bool(false));
        let flag = Flag::new(&def);
        assert!(flag.description.is_none());
    }

    #[test]
    fn test_create_many_from_flags_vector() {
        let definitions: Vec<FlagDefinition> = vec![
            ('q', "quiet", "silences the output", Value::Bool(false)),
            ('c', "config", "supply config file", Value::List(Vec::new())),
            ('h', "help", "get some help", Value::Bool(false)),
            ('v', "verbose", "level of verbosity", Value::Bool(false)),
            ('r', "rev", "revision hash", Value::Str("".to_string())),
        ];

        let flags = Flag::from_flags(&definitions);

        assert_eq!(flags.len(), definitions.len());
    }

    #[test]
    fn test_create_with_empty_definition_collection() {
        let definitions: Vec<FlagDefinition> = Vec::new();
        let flags = Flag::from_flags(&definitions);

        assert_eq!(flags.len(), 0);
    }

    #[test]
    fn test_create_parser() {
        let defs = definitions();
        let flags = Flag::from_flags(&defs);
        let parser = Parser::new(&flags);

        assert!(parser.short_map.get(&'v').is_some());
        assert!(parser.short_map.get(&'h').is_some());
        assert!(parser.short_map.get(&'c').is_some());
        assert!(parser.short_map.get(&'q').is_some());

        assert!(parser.long_map.get("verbose").is_some());
        assert!(parser.long_map.get("help").is_some());
        assert!(parser.long_map.get("config").is_some());
        assert!(parser.long_map.get("quiet").is_some());

        assert!(parser.short_map.get(&'t').is_none());
        assert!(parser.long_map.get("random").is_none());
    }

    #[test]
    fn test_parse_single_no_value_flag() {
        let definition = ('q', "quiet", "silences the output", Value::Bool(false));
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();

        let args = vec!["-q"];

        let _ = parser
            .parse_single_hyphen_flag(&mut args.into_iter().peekable(), &mut opts)
            .unwrap();
        let quiet: bool = opts.get("quiet").unwrap().clone().try_into().unwrap();
        assert!(quiet);
    }

    #[test]
    fn test_parse_single_value_flag() {
        let definition = (
            'c',
            "config",
            "supply config file",
            Value::Str("".to_string()),
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();
        const PATH: &str = "$HOME/path/to/config/file";

        let args = vec!["-c", PATH];

        let _result = parser.parse_single_hyphen_flag(&mut args.into_iter().peekable(), &mut opts);
    }

    #[test]
    fn test_parse_single_cluster_with_end_value() {
        let defs = definitions();

        let flags = Flag::from_flags(&defs);
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();
        const PATH: &str = "$HOME/path/to/config/file";
        const CLUSTER: &str = "-qhvc";

        let clustered_args = vec![CLUSTER, PATH];

        let _ = parser
            .parse_single_hyphen_flag(&mut clustered_args.into_iter().peekable(), &mut opts)
            .unwrap();

        //assert_eq!(v.len(), CLUSTER.len() - 1);
    }

    #[test]
    fn test_parse_long_single_no_value() {
        let definition = ('q', "quiet", "silences the output", Value::Bool(false));
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();

        let args = vec!["--quiet"];

        let _ = parser
            .parse_double_hyphen_flag(&mut args.into_iter().peekable(), &mut opts)
            .unwrap();

        //assert_eq!(parsed_flag, flag.long_name);
    }

    #[test]
    fn test_parse_long_single_with_value() {
        let definition = (
            'c',
            "config",
            "supply config file",
            Value::Str("".to_string()),
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();
        const PATH: &str = "$HOME/path/to/config/file";

        let args = vec!["--config", PATH];

        let _ = parser
            .parse_double_hyphen_flag(&mut args.into_iter().peekable(), &mut opts)
            .unwrap();

        //assert_eq!(parsed_flag, flag.long_name);
        //let s: String = value.clone().try_into().unwrap();
        //assert_eq!(s, PATH.to_string());
    }

    #[test]
    fn test_parse_long_single_int_value() {
        let definition = ('n', "number", "supply a number", Value::Int(0));
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();

        let args = vec!["--number", "60"];

        let _ = parser
            .parse_double_hyphen_flag(&mut args.into_iter().peekable(), &mut opts)
            .unwrap();

        //assert_eq!(parsed_flag, flag.long_name);
        //let i: i64 = value.clone().try_into().unwrap();
        //assert_eq!(i, 60);
    }

    #[test]
    fn test_parse_long_single_list_value() {
        let definition = (
            'n',
            "number",
            "supply a list of numbers",
            Value::List(Vec::new()),
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);

        let args = vec![
            "--number".to_string(),
            "60".to_string(),
            "--number".to_string(),
            "59".to_string(),
            "--number".to_string(),
            "3".to_string(),
        ];

        let result = parser.parse_args(&args).unwrap();

        let list: Vec<String> = result.get("number").unwrap().clone().try_into().unwrap();

        assert_eq!(list, vec!["60", "59", "3"]);
    }

    #[test]
    fn test_parse_long_and_short_single_list_value() {
        let definition = (
            'n',
            "number",
            "supply a list of numbers",
            Value::List(Vec::new()),
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--number", "60", "--number", "59", "-n", "3", "-n5"]);

        let result = parser.parse_args(&args).unwrap();

        let list: Vec<String> = result.get("number").unwrap().clone().try_into().unwrap();

        assert_eq!(list, vec!["60", "59", "3", "5"]);
    }

    #[test]
    fn test_parse_cluster_with_attached_value() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["-qhvcPATH/TO/FILE"]);

        let result = parser.parse_args(&args).unwrap();

        let config_path: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();

        assert!(result.opts.get("quiet").is_some());
        assert!(result.opts.get("help").is_some());
        assert!(result.opts.get("verbose").is_some());

        assert_eq!(config_path[0], "PATH/TO/FILE".to_string());
    }

    #[test]
    fn test_parse_cluster_with_attached_value_first() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["-cqhv"]);

        let result = parser.parse_args(&args).unwrap();

        let config_path: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();

        assert!(result.get("quiet").is_some());
        assert!(result.get("help").is_some());
        assert!(result.get("verbose").is_some());

        assert_eq!(config_path[0], "qhv".to_string());
    }

    #[test]
    fn test_parse_after_double_hyphen() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["-q", "--", "-v", "--", "-h"]);

        let result = parser.parse_args(&args).unwrap();

        assert!(result.get("quiet").is_some());
        assert!(result.get("verbose").is_some());
        assert!(result.get("help").is_some());

        let pos_args = vec!["-v", "--", "-h"];

        assert_eq!(pos_args, result.args);
    }

    #[test]
    fn test_parse_equals_in_value() {
        let definition = (
            'c',
            "config",
            "supply a config file",
            Value::Str("".to_string()),
        );

        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--config=--config=foo.bar"]);

        let result = parser.parse_args(&args).unwrap();

        let config_val: String = result.get("config").unwrap().clone().try_into().unwrap();

        assert_eq!("--config=foo.bar", config_val);
    }

    #[test]
    fn test_parse_list_equals_in_values() {
        let definition = (
            'c',
            "config",
            "supply multiple config files",
            Value::List(Vec::new()),
        );

        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);

        let args = create_args(vec![
            "--config=--config=foo.bar",
            "--config",
            "-c=some.value.long",
            "--config=--config=bar.foo",
        ]);

        let result = parser.parse_args(&args).unwrap();

        let config_values: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();

        assert_eq!(
            config_values,
            create_args(vec![
                "--config=foo.bar",
                "-c=some.value.long",
                "--config=bar.foo"
            ])
        );
    }

    #[test]
    fn test_parse_list_short_name_with_equals_in_value() {
        let definition = (
            'c',
            "config",
            "supply multiple config files",
            Value::Str("".to_string()),
        );

        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);

        let args = create_args(vec!["-c=--config.prop=63"]);

        let result = parser.parse_args(&args).unwrap();

        let config_value: String = result.get("config").unwrap().clone().try_into().unwrap();

        assert_eq!(config_value, "=--config.prop=63");
    }

    #[test]
    fn test_parse_list_mixed_with_spaces_and_equals() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec![
            "log",
            "--rev",
            ".",
            "--config=--rev=e45ab",
            "-c",
            "--rev=test",
            "--",
            "arg",
        ]);

        let result = parser.parse_args(&args).unwrap();

        let config_values: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();

        let rev_value: String = result.get("rev").unwrap().clone().try_into().unwrap();

        assert_eq!(config_values, vec!["--rev=e45ab", "--rev=test"]);

        assert_eq!(rev_value, ".");
    }

    #[test]
    fn test_parse_flag_with_value_last_token() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--rev"]);

        let result = parser.parse_args(&args).unwrap();

        let rev_value: String = result.get("rev").unwrap().clone().try_into().unwrap();

        assert_eq!(rev_value, "");
        // TODO for now this is expected to be the default flag val, but later a Value
        // expecting flag probably should error for the user perhaps -- depends on the current
        // CLI parsing
    }

    #[test]
    fn test_template_value_long_str_value() {
        let definition = (
            'T',
            "template",
            "specify a template",
            Value::Str("".to_string()),
        );

        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);

        let template_str = "hg bookmark -ir {node} {tag};\\n";
        // target command is `hg tags -T "hg bookmark -ir {node} {tag};\n"`
        // taken from hg/tests/test-rebase-bookmarks.t

        let args = create_args(vec!["tags", "-T", template_str]);

        let result = parser.parse_args(&args).unwrap();

        let template_val: String = result.get("template").unwrap().clone().try_into().unwrap();

        assert_eq!(template_val, template_str);
    }

    #[test]
    #[should_panic(expected = "Only Value::List can convert to List")]
    fn test_type_mismatch_try_into_list_panics() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--rev", "test"]);

        let result = parser.parse_args(&args).unwrap();

        let _: Vec<String> = result.get("rev").unwrap().clone().try_into().unwrap();
        // This is either a definition error (incorrectly configured) or
        // a programmer error at the callsite ( mismatched types ).
    }

    #[test]
    #[should_panic(expected = "Only Value::Str can convert to String")]
    fn test_type_mismatch_try_into_str_panics() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--config", "some value"]);

        let result = parser.parse_args(&args).unwrap();

        let _: String = result.get("config").unwrap().clone().try_into().unwrap();
        // This is either a definition error (incorrectly configured) or
        // a programmer error at the callsite ( mismatched types ).
    }

    #[test]
    #[should_panic(expected = "Only Value::Int can convert to i64")]
    fn test_type_mismatch_try_into_int_panics() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--rev", "test"]);

        let result = parser.parse_args(&args).unwrap();

        let _: i64 = result.get("rev").unwrap().clone().try_into().unwrap();
        // This is either a definition error (incorrectly configured) or
        // a programmer error at the callsite ( mismatched types ).
    }

    #[test]
    #[should_panic(expected = "Only Value::Bool can convert to bool")]
    fn test_type_mismatch_try_into_bool_panics() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--rev", "test"]);

        let result = parser.parse_args(&args).unwrap();

        let _: bool = result.get("rev").unwrap().clone().try_into().unwrap();
        // This is either a definition error (incorrectly configured) or
        // a programmer error at the callsite ( mismatched types ).
    }

    #[test]
    fn test_trailing_equals_sign_double_flag() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--config="]);

        let result = parser.parse_args(&args).unwrap();

        let configs: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs.get(0).unwrap(), "");
    }

}
