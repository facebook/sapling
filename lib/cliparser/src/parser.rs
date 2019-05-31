// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
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
#[cfg(test)]
mod tests {
    use super::*;

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
}
