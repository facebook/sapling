// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use std::collections::HashMap;

/// FlagDefinition represents a tuple of options that represent
/// a single definition of a flag configured by each property.
///
/// | Type         | Meaning |
/// | ---          | --- |
/// | char         | short_name of a flag i.e. '-q' |
/// | &str         | long_name of a flag i.e. '--quiet' |
/// | &str         | description of a flag i.e. 'silences the output' |
/// | ValueType    | what type of value this flag supports, or NoValue for a flag without value |
/// | Multiplicity | should this flag support being used multiple times or not i.e. `-c 1 -c 2` or `-vvv`|
///
/// To omit a short_name, pass in empty character ' '
///
/// To omit a long_name, pass in a blank string or a string with just whitespace
///
/// To omit a description, pass in a blank string or a string with just whitespace
///
/// ```
/// use cliparser::parser::{ValueType, Multiplicity, FlagDefinition};
///
/// let def: FlagDefinition = ('q',
///     "quiet",
///     "silences the output",
///     ValueType::NoValue,
///     Multiplicity::Singular);
/// ```
pub type FlagDefinition = (char, &'static str, &'static str, ValueType, Multiplicity);
/// ValueType signals if a flag accepts a value ( ValueType::Value ) or accepts no value.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum ValueType {
    NoValue,
    Value,
    RequiredValue,
}

/// Multiplicity is the enumeration of the amount of expected flags
/// during parsing.
///
/// For example:
///
/// - Singular would expect a flag to appear *at most* once.
/// - Multiple would expect a flag to appear 0..N times.
/// - Required would expect a flag to appear *at least* once.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Multiplicity {
    Singular,
    Multiple,
}

/// Flag holds information about a configurable flag to be used during parsing CLI args.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Flag<'a> {
    /// short_name of a flag i.e. `q`
    short_name: Option<char>,
    /// long_name of a flag i.e. `quiet`
    long_name: Option<&'a str>,
    /// description of a flag i.e. `silences the output`
    description: Option<&'a str>,
    /// what value this flag supports, or NoValue for none
    value_type: &'a ValueType,
    /// how many multiples of this flag are allowed to be present
    multiplicity: &'a Multiplicity,
}

impl<'a> Flag<'a> {
    /// Create a new Flag struct from a given FlagDefinition.
    ///
    /// ```
    /// use cliparser::parser::*;
    /// let def = ('q', "quiet", "silences the output", ValueType::NoValue, Multiplicity::Singular);
    /// let flag = Flag::new(&def);
    /// ```
    ///
    /// If no short_name should be used, provide an empty char ' '
    /// ```
    /// use cliparser::parser::*;
    /// let def = (' ', "quiet", "silences the output", ValueType::NoValue, Multiplicity::Singular);
    /// ```
    ///
    /// If no long_name should be used, provide an empty string
    /// ```
    /// use cliparser::parser::*;
    /// let def = ('q', "", "silences the output", ValueType::NoValue, Multiplicity::Singular);
    /// ```
    ///
    /// If no description should be used, provide an empty string
    /// ```
    /// use cliparser::parser::*;
    /// let def = ('q', "quiet", "", ValueType::NoValue, Multiplicity::Singular);
    /// ```
    ///
    /// If both short_name and long_name are empty, new will panic as this would create
    /// an ambiguous flag
    ///
    /// ```should_panic
    /// use cliparser::parser::*;
    /// let def = (' ', "", "ambiguous description", ValueType::NoValue, Multiplicity::Singular);
    /// let flag = Flag::new(&def); // panic caused by ambiguous flag
    /// ```
    pub fn new(definition: &'a FlagDefinition) -> Self {
        let short_name_opt = match definition.0 {
            ' ' => None,
            _ => Some(definition.0),
        };

        let long_name_opt = match definition.1 {
            long_name if long_name.is_empty() => None,
            _ => Some(definition.1),
        };

        assert!(
            short_name_opt.is_some() || long_name_opt.is_some(),
            "Flag definition had neither short_name nor long_name which makes it ambiguous!"
        );

        let description_opt = match definition.2 {
            description if description.is_empty() => None,
            _ => Some(definition.2),
        };

        let value_type = &definition.3;
        let multiplicity = &definition.4;

        Flag {
            short_name: short_name_opt,
            long_name: long_name_opt,
            description: description_opt,
            value_type,
            multiplicity,
        }
    }

    /// Create a vector of Flags from a collection of FlagDefinition.
    ///
    /// ```
    /// use cliparser::parser::*;
    ///
    /// let defs: Vec<FlagDefinition> = vec![
    /// ('q', "quiet", "silences the output", ValueType::NoValue, Multiplicity::Singular),
    /// ('c', "config", "supply config file", ValueType::Value, Multiplicity::Singular),
    /// ('h', "help", "get some help", ValueType::NoValue, Multiplicity::Singular),
    /// ('v', "verbose", "level of verbosity", ValueType::NoValue, Multiplicity::Multiple),
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
}

type FlagValuePair<'a> = (&'a Flag<'a>, Option<&'a str>);

impl<'a> Parser<'a> {
    /// initialize and setup a parser with all known flag definitions
    /// ```
    /// use cliparser::parser::*;
    ///
    /// let definitions = vec![
    /// ('c', "config", "supply a config", ValueType::Value, Multiplicity::Singular),
    /// ('h', "help", "get some help", ValueType::NoValue, Multiplicity::Singular),
    /// ('q', "quiet", "silence the output", ValueType::NoValue, Multiplicity::Singular)
    /// ];
    ///
    /// let flags = Flag::from_flags(&definitions);
    /// let parser = Parser::new(&flags);
    /// ```
    pub fn new(flags: &'a Vec<Flag<'a>>) -> Self {
        let mut short_map: HashMap<&'a char, &'a Flag<'a>> = HashMap::new();
        let mut long_map: HashMap<&'a str, &'a Flag<'a>> = HashMap::new();

        for flag in flags {
            if let Some(ref character) = flag.short_name {
                short_map.insert(character, &flag);
            }

            if let Some(ref long_name) = flag.long_name {
                long_map.insert(long_name, &flag);
            }
        }

        Parser {
            short_map,
            long_map,
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
    /// ('q', "quiet", "silence the output", ValueType::NoValue, Multiplicity::Singular)
    /// ];
    ///
    /// let flags = Flag::from_flags(&definitions);
    /// let parser = Parser::new(&flags);
    ///
    /// parser.parse_args(&env_args);
    /// ```
    ///
    /// parse_args will clean arguments such that they can be properly parsed by Parser#_parse
    pub fn parse_args(&self, args: &'a Vec<String>) -> Result<ParseOutput, &'static str> {
        let arg_vec: Vec<&'a str> = args
            .iter()
            .skip(1)
            .flat_map(|arg| arg.split("="))
            .map(|arg| arg.trim())
            .filter(|arg| !arg.is_empty())
            .collect();

        self._parse(arg_vec)
    }

    pub fn _parse(&self, args: Vec<&'a str>) -> Result<ParseOutput, &'static str> {
        let mut iter = args.into_iter().peekable();
        let mut found_flag_pairs: Vec<FlagValuePair> = Vec::new();
        let mut positional_args = Vec::new();
        let mut unknown_args = Vec::new();

        while let Some(&arg) = iter.peek() {
            if arg.eq("--") {
                positional_args.extend(iter);
                break;
            } else if arg.starts_with("--") {
                match self.parse_double_hyphen_flag(&mut iter) {
                    Ok(flag_value_pair) => found_flag_pairs.push(flag_value_pair),
                    Err(msg) => {
                        println!("error {}", msg); // TODO implement actual error handling with Fallible
                        unknown_args.push(arg);
                    }
                }
            } else if arg.starts_with("-") {
                match self.parse_single_hyphen_flag(&mut iter) {
                    Ok(ref mut flag_value_pairs) => found_flag_pairs.append(flag_value_pairs),
                    Err(msg) => {
                        println!("error {}", msg); // TODO implement actual error handling with Fallible
                        unknown_args.push(arg);
                    }
                }
            } else {
                positional_args.push(arg);
                iter.next();
            }
        }

        Ok(self.build_parse_result(positional_args, found_flag_pairs))
    }

    fn build_parse_result(
        &self,
        positional_args: Vec<&'a str>,
        flag_pairs: Vec<FlagValuePair<'a>>,
    ) -> ParseOutput<'a> {
        let mut parsed_result = ParseOutput::new();

        for (found_flag, value_opt) in flag_pairs {
            match found_flag.multiplicity {
                Multiplicity::Singular => {
                    if let Some(ref val) = value_opt {
                        parsed_result.value_map.entry(found_flag).or_insert(val);
                    }
                }
                Multiplicity::Multiple => {
                    if let Some(val) = value_opt {
                        parsed_result
                            .multiple_map
                            .entry(found_flag)
                            .or_insert(Vec::new())
                            .push(val);
                    }
                }
            }
        }

        parsed_result.args = positional_args;
        parsed_result.long_map = self.long_map.clone();

        parsed_result
    }

    fn parse_double_hyphen_flag(
        &self,
        iter: &mut Iterator<Item = &'a str>,
    ) -> Result<FlagValuePair, &'static str> {
        let clean_arg = iter.next().unwrap().trim_start_matches("--");

        // TODO handle prefix matching as well as --no-foo for boolean foo flag
        if let Some(known_flag) = self.long_map.get(clean_arg) {
            let val_opt = match known_flag.value_type {
                ValueType::Value | ValueType::RequiredValue => iter.next(),
                _ => None,
            };
            return Ok((*known_flag, val_opt));
        }
        Err("Could not parse a flag!")
    }

    //TODO -Tv should parse to -T 'v' instead of -T -v if -T accepts a value
    fn parse_single_hyphen_flag(
        &self,
        iter: &mut Iterator<Item = &'a str>,
    ) -> Result<Vec<FlagValuePair>, &'static str> {
        let clean_arg = iter.next().unwrap().trim_start_matches("-");
        let mut found_flag_pairs: Vec<FlagValuePair> = Vec::new();
        let mut last_letter = ' ';

        for c in clean_arg.chars() {
            last_letter = c;
            if let Some(known_flag) = self.short_map.get(&c) {
                found_flag_pairs.push((*known_flag, None));
            }
        }

        if let Some(known_flag) = self.short_map.get(&last_letter) {
            match known_flag.value_type {
                ValueType::Value | ValueType::RequiredValue => {
                    found_flag_pairs.pop();
                    found_flag_pairs.push((*known_flag, iter.next()));
                }
                _ => (),
            }
        }

        Ok(found_flag_pairs)
    }
}
// TODO refactor and remove lifetime from ParseOutput
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseOutput<'a> {
    /// The mapping of a flag to the str value
    value_map: HashMap<&'a Flag<'a>, &'a str>,
    /// The mapping of a flag to its multiple values
    multiple_map: HashMap<&'a Flag<'a>, Vec<&'a str>>,
    /// The positional args
    args: Vec<&'a str>,
    /// The mapping of a &Flag.long_name -> &Flag
    long_map: HashMap<&'a str, &'a Flag<'a>>,
}

/// ParseOutput represents all of the information successfully parsed from the command-line
/// arguments, as well as exposing a convenient API for application logic to query results
/// parsed.
impl<'a> ParseOutput<'a> {
    fn new() -> Self {
        ParseOutput {
            value_map: HashMap::new(),
            multiple_map: HashMap::new(),
            long_map: HashMap::new(),
            args: Vec::new(),
        }
    }

    /// For a given Flag.long_name, return a vector of all values parsed
    pub fn get_values(&self, long_name: &str) -> Option<Vec<&'a str>> {
        if let Some(flag) = self.long_map.get(long_name) {
            if let Some(val) = self.multiple_map.get(*flag) {
                return Some(val.clone());
            }
        }
        None
    }

    /// For a given Flag.long_name, return the single value parsed
    pub fn get_value(&self, long_name: &str) -> Option<&'a str> {
        if let Some(flag) = self.long_map.get(long_name) {
            if let Some(val) = self.value_map.get(*flag) {
                return Some(*val);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definitions() -> Vec<FlagDefinition> {
        vec![
            (
                'q',
                "quiet",
                "silences the output",
                ValueType::NoValue,
                Multiplicity::Singular,
            ),
            (
                'c',
                "config",
                "supply config file",
                ValueType::Value,
                Multiplicity::Singular,
            ),
            (
                'h',
                "help",
                "get some help",
                ValueType::NoValue,
                Multiplicity::Singular,
            ),
            (
                'v',
                "verbose",
                "level of verbosity",
                ValueType::NoValue,
                Multiplicity::Multiple,
            ),
        ]
    }

    #[test]
    fn create_1_flag() {
        let def = (
            'q',
            "quiet",
            "silences the output",
            ValueType::NoValue,
            Multiplicity::Singular,
        );
        let flag = Flag::new(&def);
        assert_eq!('q', flag.short_name.unwrap());
        assert_eq!("quiet", flag.long_name.unwrap());
        assert_eq!("silences the output", flag.description.unwrap());
        assert_eq!(&ValueType::NoValue, flag.value_type);
        assert_eq!(&Multiplicity::Singular, flag.multiplicity);
    }

    #[test]
    fn test_create_1_flag_with_empty_short_name() {
        let def = (
            ' ',
            "quiet",
            "silences the output",
            ValueType::NoValue,
            Multiplicity::Singular,
        );
        let flag = Flag::new(&def);
        assert!(flag.short_name.is_none());
    }

    #[test]
    fn test_create_1_flag_with_empty_long_name() {
        let def = (
            'q',
            "",
            "silences the output",
            ValueType::NoValue,
            Multiplicity::Singular,
        );
        let flag = Flag::new(&def);
        assert!(flag.long_name.is_none());
    }

    #[test]
    fn test_create_1_flag_with_empty_description() {
        let def = ('q', "quiet", "", ValueType::NoValue, Multiplicity::Singular);
        let flag = Flag::new(&def);
        assert!(flag.description.is_none());
    }

    #[test]
    #[should_panic(
        expected = "Flag definition had neither short_name nor long_name which makes it ambiguous!"
    )]
    fn test_create_1_flag_with_empty_long_and_short_name() {
        let def = (
            ' ',
            "",
            "some description",
            ValueType::NoValue,
            Multiplicity::Singular,
        );
        let _flag = Flag::new(&def); // this should panic because the flag is completely ambiguous
    }

    #[test]
    fn test_create_many_from_flags_vector() {
        let definitions: Vec<FlagDefinition> = vec![
            (
                'q',
                "quiet",
                "silences the output",
                ValueType::NoValue,
                Multiplicity::Singular,
            ),
            (
                'c',
                "config",
                "supply config file",
                ValueType::Value,
                Multiplicity::Singular,
            ),
            (
                'h',
                "help",
                "get some help",
                ValueType::NoValue,
                Multiplicity::Singular,
            ),
            (
                'v',
                "verbose",
                "level of verbosity",
                ValueType::NoValue,
                Multiplicity::Multiple,
            ),
        ];

        let flags = Flag::from_flags(&definitions);

        assert_eq!(flags.len(), 4);
    }

    #[test]
    fn test_create_with_empty_definition_collection() {
        let definitions: Vec<FlagDefinition> = Vec::new();
        let flags = Flag::from_flags(&definitions);

        assert_eq!(flags.len(), 0);
    }

    #[test]
    #[should_panic(
        expected = "Flag definition had neither short_name nor long_name which makes it ambiguous!"
    )]
    fn test_create_many_ambiguous_flags() {
        let definitions: Vec<FlagDefinition> = vec![
            (
                ' ',
                "",
                "random description",
                ValueType::NoValue,
                Multiplicity::Multiple,
            ),
            (
                ' ',
                "",
                "another random description",
                ValueType::NoValue,
                Multiplicity::Multiple,
            ),
            (
                'q',
                "quiet",
                "silences output",
                ValueType::NoValue,
                Multiplicity::Multiple,
            ),
        ];

        let _flags = Flag::from_flags(&definitions);
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
        let definition = (
            'q',
            "quiet",
            "silences the output",
            ValueType::NoValue,
            Multiplicity::Singular,
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);

        let args = vec!["-q"];

        let v = parser
            .parse_single_hyphen_flag(&mut args.into_iter().peekable())
            .unwrap();
        assert_eq!(v.len(), 1);
        let (parsed_flag, value_opt) = v[0];
        assert_eq!(*parsed_flag, flag);
        assert!(value_opt.is_none());
    }

    #[test]
    fn test_parse_single_value_flag() {
        let definition = (
            'c',
            "config",
            "supply config file",
            ValueType::Value,
            Multiplicity::Singular,
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        const PATH: &str = "$HOME/path/to/config/file";

        let args = vec!["-c", PATH];
        // TODO test -cPATH, -vqcPATH, -vcq, -cqv, -c -q

        let result = parser.parse_single_hyphen_flag(&mut args.into_iter().peekable());

        match result {
            Ok(v) => {
                assert_eq!(v.len(), 1);
                let (parsed_flag, value_opt) = v[0];
                assert_eq!(*parsed_flag, flag);
                assert!(value_opt.is_some());
                assert_eq!(value_opt.unwrap(), PATH)
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn test_parse_cluster_no_value_flags() {
        let defs = definitions();

        let flags = Flag::from_flags(&defs);
        let parser = Parser::new(&flags);

        let clustered_args = vec!["-qhv"]; // clustered should equal "-q -h -h"

        let unclustered_args = vec!["-q", "-h", "-v"];

        let clustered_result =
            parser.parse_single_hyphen_flag(&mut clustered_args.into_iter().peekable());

        let mut unclustered_vec: Vec<FlagValuePair> = Vec::new();

        let mut iter = unclustered_args.into_iter().peekable();

        while let Some(&_) = iter.peek() {
            unclustered_vec.append(&mut parser.parse_single_hyphen_flag(&mut iter).unwrap());
        }

        assert_eq!(clustered_result.unwrap(), unclustered_vec);
    }

    #[test]
    fn test_parse_single_cluster_with_end_value() {
        let defs = definitions();

        let flags = Flag::from_flags(&defs);
        let parser = Parser::new(&flags);
        const PATH: &str = "$HOME/path/to/config/file";
        const CLUSTER: &str = "-qhvc";

        let clustered_args = vec![CLUSTER, PATH];

        let v = parser
            .parse_single_hyphen_flag(&mut clustered_args.into_iter().peekable())
            .unwrap();

        assert_eq!(v.len(), CLUSTER.len() - 1);

        let (_flag, value_opt) = v[v.len() - 1];

        assert!(value_opt.is_some());
        assert_eq!(value_opt.unwrap(), PATH);
    }

    #[test]
    fn test_parse_long_single_no_value() {
        let definition = (
            'q',
            "quiet",
            "silences the output",
            ValueType::NoValue,
            Multiplicity::Singular,
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);

        let args = vec!["--quiet"];

        let (parsed_flag, value_opt) = parser
            .parse_double_hyphen_flag(&mut args.into_iter().peekable())
            .unwrap();

        assert_eq!(*parsed_flag, flag);
        assert!(value_opt.is_none());
    }

    #[test]
    fn test_parse_long_single_with_value() {
        let definition = (
            'c',
            "config",
            "supply config file",
            ValueType::Value,
            Multiplicity::Singular,
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        const PATH: &str = "$HOME/path/to/config/file";

        let args = vec!["--config", PATH];
        // TODO --config=--config=foo.bar is parsed as { config : --config=foo.bar }

        let (parsed_flag, value_opt) = parser
            .parse_double_hyphen_flag(&mut args.into_iter().peekable())
            .unwrap();

        assert_eq!(*parsed_flag, flag);
        assert!(value_opt.is_some());
        assert_eq!(value_opt.unwrap(), PATH)
    }
}

// TODO test -- behavior like -q -- -v -- -h
