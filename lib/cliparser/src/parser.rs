// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::utils::get_prefix_bounds;

#[cfg(feature = "python")]
use cpython::{
    FromPyObject, PyBool, PyInt, PyList, PyObject, PyResult, PyString, Python, PythonObject,
    ToPyObject,
};
#[cfg(feature = "python")]
use cpython_ext::Bytes;
use failure::Fail;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
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
///     "quiet".into(),
///     "silences the output".into(),
///     Value::Bool(false));
/// ```
pub type FlagDefinition<'a> = (char, Cow<'a, str>, Cow<'a, str>, Value);

#[derive(Debug, Fail)]
pub enum ParseError {
    #[fail(display = "option {} not recognized", option_name)]
    OptionNotRecognized { option_name: String },
    #[fail(display = "option {} requires argument", option_name)]
    OptionRequiresArgument { option_name: String },
    #[fail(
        display = "invalid value '{}' for option {}, expected {}",
        given, option_name, expected
    )]
    OptionArgumentInvalid {
        option_name: String,
        given: String,
        expected: String,
    },
    #[fail(display = "option {} not a unique prefix", option_name)]
    OptionAmbiguous {
        option_name: String,
        possibilities: Vec<String>,
    },
    #[fail(display = "Command {} is ambiguous", command_name)]
    AmbiguousCommand {
        command_name: String,
        possibilities: Vec<String>,
    },
    #[fail(display = "Alias {} resulted in a circular reference", command_name)]
    CircularReference { command_name: String },
    #[fail(display = "alias definition {} = {:?} cannot be parsed", name, value)]
    MalformedAlias { name: String, value: String },
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Value {
    OptBool(),
    Bool(bool),
    Str(String),
    Int(i64),
    List(Vec<String>),
}

impl Value {
    fn accept(&mut self, token_opt: Option<&str>) -> Result<(), ParseError> {
        let token = match token_opt {
            Some(s) => s,
            None => {
                return Err(ParseError::OptionRequiresArgument {
                    option_name: "".to_string(),
                })
            }
        };

        match self {
            Value::Bool(_) | Value::OptBool() => unreachable!(),
            Value::Str(ref mut s) => {
                *s = token.to_string();
                Ok(())
            }
            Value::Int(ref mut i) => {
                *i = token
                    .parse::<i64>()
                    .map_err(|_| ParseError::OptionArgumentInvalid {
                        option_name: "".to_string(),
                        given: token.to_string(),
                        expected: "int".to_string(),
                    })?;
                Ok(())
            }
            Value::List(ref mut vec) => {
                vec.push(token.to_string());
                Ok(())
            }
        }
    }
}

#[cfg(feature = "python")]
impl ToPyObject for Value {
    type ObjectType = PyObject;

    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        match self {
            Value::OptBool() => py.None().into_object(),
            Value::Bool(b) => b.to_py_object(py).into_object(),
            Value::Str(s) => Bytes::from(s.to_string()).to_py_object(py).into_object(),
            Value::Int(i) => i.to_py_object(py).into_object(),
            Value::List(vec) => {
                let collection: Vec<Bytes> = vec
                    .into_iter()
                    .map(|s: &String| Bytes::from(s.to_string()))
                    .collect();
                collection.to_py_object(py).into_object()
            }
        }
    }
}

#[cfg(feature = "python")]
impl<'source> FromPyObject<'source> for Value {
    fn extract(py: Python, obj: &'source PyObject) -> PyResult<Self> {
        if let Ok(b) = obj.cast_as::<PyBool>(py) {
            return Ok(Value::Bool(b.is_true()));
        }

        if let Ok(_l) = obj.cast_as::<PyList>(py) {
            return Ok(Value::List(Vec::new()));
        }
        if let Ok(s) = obj.cast_as::<PyString>(py) {
            return Ok(Value::Str(s.to_string(py).unwrap().to_string()));
        }

        if let Ok(_i) = obj.cast_as::<PyInt>(py) {
            return Ok(Value::Int(obj.extract::<i64>(py).unwrap()));
        }

        Ok(Value::OptBool())
    }
}

impl From<Value> for i64 {
    fn from(v: Value) -> Self {
        match v {
            Value::Int(i) => i,
            _ => panic!("programming error:  {:?} was converted to i64", v),
        }
    }
}

impl From<Value> for String {
    fn from(v: Value) -> Self {
        match v {
            Value::Str(s) => s,
            _ => panic!("programming error:  {:?} was converted to String", v),
        }
    }
}

impl From<Value> for bool {
    fn from(v: Value) -> Self {
        match v {
            Value::Bool(b) => b,
            _ => panic!("programming error:  {:?} was converted to bool", v),
        }
    }
}

impl From<Value> for Vec<String> {
    fn from(v: Value) -> Self {
        match v {
            Value::List(vec) => vec,
            _ => panic!("programming error:  {:?} was converted to Vec<String>", v),
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
    /// let def: FlagDefinition = ('q', "quiet".into(), "silences the output".into(), Value::Bool(false));
    /// let flag = Flag::new(&def);
    /// ```
    ///
    /// If no short_name should be used, provide an empty char ' '
    /// ```
    /// use cliparser::parser::*;
    /// let def: FlagDefinition = (' ', "quiet".into(), "silences the output".into(), Value::Bool(false));
    /// ```
    ///
    /// If no description should be used, provide an empty string
    /// ```
    /// use cliparser::parser::*;
    /// let def: FlagDefinition = ('q', "quiet".into(), "".into(), Value::Bool(false));
    /// ```
    ///
    pub fn new(definition: &'a FlagDefinition) -> Self {
        let short_name_opt = match definition.0 {
            ' ' => None,
            _ => Some(definition.0),
        };

        let long_name = definition.1.as_ref();

        let description = Some(definition.2.as_ref());

        let value_type = &definition.3;

        Flag {
            short_name: short_name_opt,
            long_name,
            description,
            value_type,
        }
    }

    /// Create a vector of Flags from a collection of FlagDefinition.
    ///
    /// ```
    /// use cliparser::parser::*;
    ///
    /// let defs: Vec<FlagDefinition> = vec![
    /// ('q', "quiet".into(), "silences the output".into(), Value::Bool(false)),
    /// ('c', "config".into(), "supply config file".into(), Value::Str("".to_string())),
    /// ('h', "help".into(), "get some help".into(), Value::Bool(false)),
    /// ('v', "verbose".into(), "level of verbosity".into(), Value::Bool(false)),
    /// ];
    ///
    /// let flags = Flag::from_flags(&defs);
    /// assert_eq!(flags.len(), 4);
    /// ```
    pub fn from_flags(definitions: &'a [FlagDefinition]) -> Vec<Flag<'a>> {
        definitions.iter().map(|def| Flag::new(def)).collect()
    }
}

pub struct ParseOptions {
    ignore_prefix: bool,
    ignore_errors: bool,
    early_parse: bool,
    keep_sep: bool,
    error_on_unknown_opts: bool,
    flag_aliases: HashMap<String, String>,
}

impl ParseOptions {
    pub fn new() -> Self {
        ParseOptions {
            ignore_prefix: false,
            ignore_errors: false,
            early_parse: false,
            keep_sep: false,
            error_on_unknown_opts: false,
            flag_aliases: HashMap::new(),
        }
    }

    pub fn ignore_prefix(mut self, ignore_prefix: bool) -> Self {
        self.ignore_prefix = ignore_prefix;
        self
    }

    pub fn ignore_errors(mut self, ignore_errors: bool) -> Self {
        self.ignore_errors = ignore_errors;
        self
    }

    pub fn early_parse(mut self, early_parse: bool) -> Self {
        self.early_parse = early_parse;
        self
    }

    pub fn keep_sep(mut self, keep_sep: bool) -> Self {
        self.keep_sep = keep_sep;
        self
    }

    pub fn flag_alias(mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.flag_aliases
            .insert(key.as_ref().to_string(), value.as_ref().to_string());
        self
    }

    pub fn error_on_unknown_opts(mut self, error_on_unknown_opts: bool) -> Self {
        self.error_on_unknown_opts = error_on_unknown_opts;
        self
    }
}

/// [`Parser`] keeps flag definitions and uses them to parse string arguments.
pub struct Parser<'a> {
    /// map holding &character -> &flag where the character == flag.short_name
    short_map: HashMap<&'a char, &'a Flag<'a>>,
    /// map holding &str -> &flag where the str == flag.long_name
    long_map: BTreeMap<String, &'a Flag<'a>>,
    opts: HashMap<String, Value>,
    parsing_options: ParseOptions,
}

impl<'a> Parser<'a> {
    /// initialize and setup a parser with all known flag definitions
    /// ```
    /// use cliparser::parser::*;
    ///
    /// let definitions: Vec<FlagDefinition> = vec![
    /// ('c', "config".into(), "supply a config".into(), Value::Bool(false)),
    /// ('h', "help".into(), "get some help".into(), Value::Bool(false)),
    /// ('q', "quiet".into(), "silence the output".into(), Value::Bool(false))
    /// ];
    ///
    /// let flags = Flag::from_flags(&definitions);
    /// let parser = Parser::new(&flags);
    /// ```
    pub fn new(flags: &'a Vec<Flag<'a>>) -> Self {
        let mut short_map = HashMap::new();
        let mut long_map = BTreeMap::new();
        let mut opts = HashMap::new();

        for flag in flags {
            if let Some(ref character) = flag.short_name {
                short_map.insert(character, flag);
            }

            long_map.insert(flag.long_name.to_string(), flag);
            let value_opt = match flag.value_type {
                _ => flag.value_type.clone(),
            };
            opts.insert(flag.long_name.to_string(), value_opt);
        }

        Parser {
            short_map,
            long_map,
            opts,
            parsing_options: ParseOptions::new(),
        }
    }

    pub fn with_parsing_options(mut self, parsing_options: ParseOptions) -> Self {
        self.parsing_options = parsing_options;
        self
    }

    /// entry-point for parsing command line arguments from std::env
    ///
    /// ```
    /// use std::env;
    /// use cliparser::parser::*;
    ///
    /// let env_args = env::args().collect();
    ///
    /// let definitions: Vec<FlagDefinition> = vec![
    /// ('q', "quiet".into(), "silence the output".into(), Value::Bool(false))
    /// ];
    ///
    /// let flags = Flag::from_flags(&definitions);
    /// let parser = Parser::new(&flags);
    ///
    /// parser.parse_args(&env_args);
    /// ```
    ///
    /// parse_args will clean arguments such that they can be properly parsed by Parser#_parse
    pub fn parse_args(&self, args: &'a Vec<impl AsRef<str>>) -> Result<ParseOutput, ParseError> {
        let args: Vec<&'a str> = args.iter().map(AsRef::as_ref).collect();

        let mut first_arg_index = args.len();
        let mut opts = self.opts.clone();
        let mut iter = args.into_iter().enumerate().peekable();
        let mut positional_args = Vec::new();

        let mut set_first_arg_index = |positional_args: &Vec<&str>, i| {
            if positional_args.is_empty() {
                first_arg_index = i;
            }
        };

        while let Some(&(i, arg)) = iter.peek() {
            if arg.eq("--") {
                if !self.parsing_options.keep_sep {
                    let _ = iter.next(); // don't care about -- it's just a separator
                }
                set_first_arg_index(&positional_args, i);
                positional_args.extend(iter.map(|(_i, arg)| arg));
                break;
            } else if arg.eq("-") {
                set_first_arg_index(&positional_args, i);
                positional_args.push(arg);
                iter.next();
            } else if arg.starts_with("--") {
                if let Err(msg) = self.parse_double_hyphen_flag(&mut iter, &mut opts) {
                    if self.parsing_options.error_on_unknown_opts {
                        return Err(msg);
                    } else {
                        set_first_arg_index(&positional_args, i);
                        positional_args.push(arg);
                    }
                }
            } else if arg.starts_with("-") {
                if let Err(msg) = self.parse_single_hyphen_flag(&mut iter, &mut opts) {
                    if self.parsing_options.error_on_unknown_opts {
                        return Err(msg);
                    } else {
                        set_first_arg_index(&positional_args, i);
                        positional_args.push(arg);
                    }
                }
            } else {
                set_first_arg_index(&positional_args, i);
                positional_args.push(arg);
                iter.next();
            }
        }

        Ok(ParseOutput::new(
            opts,
            positional_args.iter().map(|s| s.to_string()).collect(),
            first_arg_index,
        ))
    }

    fn parse_double_hyphen_flag(
        &self,
        iter: &mut impl Iterator<Item = (usize, &'a str)>,
        opts: &mut HashMap<String, Value>,
    ) -> Result<(), ParseError> {
        let arg = iter.next().unwrap().1;
        debug_assert!(arg.starts_with("--"));
        let arg = &arg[2..];
        let (arg, positive_flag) = if arg.starts_with("no-") {
            (&arg[3..], false)
        } else {
            (arg, true)
        };

        let mut parts = arg.splitn(2, "=");
        let clean_arg = parts.next().unwrap();
        let clean_arg = self
            .parsing_options
            .flag_aliases
            .get(clean_arg)
            .map(|name| name.as_ref())
            .unwrap_or(clean_arg);

        if let Some(known_flag) = self.long_map.get(clean_arg) {
            match opts.get_mut(known_flag.long_name) {
                Some(Value::OptBool()) => {
                    opts.insert(known_flag.long_name.to_string(), Value::Bool(positive_flag));
                }
                Some(Value::Bool(ref mut b)) => *b = positive_flag,
                Some(ref mut value) => {
                    let next = parts.next().or_else(|| iter.next().map(|(_i, arg)| arg));
                    value
                        .accept(next)
                        .map_err(|e| Parser::inject_option_name("--", known_flag.long_name, e))?;
                }
                None => unreachable!(),
            }
            return Ok(());
        };

        let flag_with_no: String = "no-".to_string() + clean_arg;

        if let Some(known_flag) = self.long_map.get(&flag_with_no) {
            match opts.get_mut(known_flag.long_name) {
                Some(Value::OptBool()) => {
                    opts.insert(
                        known_flag.long_name.to_string(),
                        Value::Bool(!positive_flag),
                    );
                }
                Some(Value::Bool(ref mut b)) => *b = !positive_flag,
                Some(ref mut value) => {
                    let next = parts.next().or_else(|| iter.next().map(|(_i, arg)| arg));
                    value
                        .accept(next)
                        .map_err(|e| Parser::inject_option_name("--", known_flag.long_name, e))?;
                }
                None => unreachable!(),
            }
            return Ok(());
        }

        if self.parsing_options.ignore_prefix {
            return Err(ParseError::OptionNotRecognized {
                option_name: "--".to_owned() + clean_arg,
            });
        }

        let range = self.long_map.range(get_prefix_bounds(clean_arg));
        let prefixed_flags: Vec<&Flag> = range.map(|(_, flag)| *flag).collect();

        if prefixed_flags.len() > 1 {
            return Err(ParseError::OptionAmbiguous {
                option_name: "--".to_owned() + clean_arg,
                possibilities: prefixed_flags
                    .iter()
                    .map(|f| f.long_name.to_string())
                    .collect(),
            });
        } else if prefixed_flags.len() == 0 {
            return Err(ParseError::OptionNotRecognized {
                option_name: "--".to_owned() + clean_arg,
            });
        } else {
            let matched_flag = prefixed_flags.get(0).unwrap();
            match opts.get_mut(matched_flag.long_name) {
                Some(Value::OptBool()) => {
                    opts.insert(
                        matched_flag.long_name.to_string(),
                        Value::Bool(positive_flag),
                    );
                }
                Some(Value::Bool(ref mut b)) => *b = positive_flag,
                Some(ref mut value) => {
                    let next = parts.next().or_else(|| iter.next().map(|(_i, arg)| arg));
                    value
                        .accept(next)
                        .map_err(|e| Parser::inject_option_name("--", matched_flag.long_name, e))?;
                }
                None => unreachable!(),
            }
            return Ok(());
        }
    }

    fn parse_single_hyphen_flag(
        &self,
        iter: &mut impl Iterator<Item = (usize, &'a str)>,
        opts: &mut HashMap<String, Value>,
    ) -> Result<(), ParseError> {
        let clean_arg = iter.next().unwrap().1.trim_start_matches("-");

        let mut char_iter = clean_arg.chars().peekable();

        while let Some(curr_char) = char_iter.next() {
            if let Some(known_flag) = self.short_map.get(&curr_char) {
                let flag_name = known_flag.long_name.to_string();
                match opts.get_mut(&flag_name) {
                    Some(Value::OptBool()) => {
                        opts.insert(flag_name, Value::Bool(true));
                    }
                    Some(Value::Bool(ref mut b)) => *b = true,
                    Some(ref mut value) => {
                        if char_iter.peek().is_none() {
                            let next = iter.next().map(|(_i, arg)| arg);
                            value.accept(next).map_err(|e| {
                                Parser::inject_option_name("-", curr_char.to_string().as_ref(), e)
                            })?;
                        } else {
                            let consumed = char_iter.collect::<String>();
                            let consumed = Some(&consumed[..]);
                            value.accept(consumed).map_err(|e| {
                                Parser::inject_option_name("-", curr_char.to_string().as_ref(), e)
                            })?;
                            break;
                        }
                    }
                    None => unreachable!(),
                }
            } else {
                return Err(ParseError::OptionNotRecognized {
                    option_name: "-".to_string() + curr_char.to_string().as_ref(),
                });
            }
            if self.parsing_options.early_parse {
                break;
            }
        }
        Ok(())
    }

    fn inject_option_name(prefix: &str, name: &str, error: ParseError) -> ParseError {
        match error {
            ParseError::OptionNotRecognized { option_name: _ } => ParseError::OptionNotRecognized {
                option_name: prefix.to_string() + name,
            },
            ParseError::OptionRequiresArgument { option_name: _ } => {
                ParseError::OptionRequiresArgument {
                    option_name: prefix.to_string() + name,
                }
            }
            ParseError::OptionArgumentInvalid {
                option_name: _,
                given,
                expected,
            } => ParseError::OptionArgumentInvalid {
                option_name: prefix.to_string() + name,
                given,
                expected,
            },
            ParseError::OptionAmbiguous {
                option_name: _,
                possibilities,
            } => ParseError::OptionAmbiguous {
                option_name: prefix.to_string() + name,
                possibilities,
            },
            err => err,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseOutput {
    /// The opts
    opts: HashMap<String, Value>,
    /// The positional args
    args: Vec<String>,
    first_arg_index: usize,
}

/// ParseOutput represents all of the information successfully parsed from the command-line
/// arguments, as well as exposing a convenient API for application logic to query results
/// parsed.
impl ParseOutput {
    pub fn new(opts: HashMap<String, Value>, args: Vec<String>, first_arg_index: usize) -> Self {
        ParseOutput {
            opts,
            args,
            first_arg_index,
        }
    }

    pub fn get(&self, long_name: &str) -> Option<&Value> {
        self.opts.get(long_name)
    }

    pub fn get_or_default<T>(&self, long_name: &str, default: T) -> T
    where
        T: std::convert::From<Value>,
    {
        self.opts
            .get(long_name)
            .map(|v| v.clone())
            .and_then(|v| v.try_into().ok())
            .unwrap_or(default)
    }

    pub fn opts(&self) -> &HashMap<String, Value> {
        &self.opts
    }

    pub fn args(&self) -> &Vec<String> {
        &self.args
    }

    /// The index of the first positional argument in the original arguments
    /// passed to `Parser::parse_args`.
    /// If there are no positional arguments, return the length of the original
    /// arguments.
    pub fn first_arg_index(&self) -> usize {
        self.first_arg_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definitions() -> Vec<FlagDefinition<'static>> {
        vec![
            (
                'q',
                "quiet".into(),
                "silences the output".into(),
                Value::Bool(false),
            ),
            (
                'c',
                "config".into(),
                "supply config file".into(),
                Value::List(Vec::new()),
            ),
            (
                'h',
                "help".into(),
                "get some help".into(),
                Value::Bool(false),
            ),
            (
                'v',
                "verbose".into(),
                "level of verbosity".into(),
                Value::Bool(false),
            ),
            (
                'r',
                "rev".into(),
                "revision hash".into(),
                Value::Str("".to_string()),
            ),
        ]
    }

    fn create_args(strings: Vec<&str>) -> Vec<String> {
        strings.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_create_1_flag() {
        let def = (
            'q',
            "quiet".into(),
            "silences the output".into(),
            Value::Bool(false),
        );
        let flag = Flag::new(&def);
        assert_eq!('q', flag.short_name.unwrap());
        assert_eq!("quiet", flag.long_name);
        assert_eq!("silences the output", flag.description.unwrap());
        assert_eq!(&Value::Bool(false), flag.value_type);
    }

    #[test]
    fn test_create_1_flag_with_empty_short_name() {
        let def = (
            ' ',
            "quiet".into(),
            "silences the output".into(),
            Value::Bool(false),
        );
        let flag = Flag::new(&def);
        assert!(flag.short_name.is_none());
    }

    #[test]
    fn test_create_many_from_flags_vector() {
        let definitions: Vec<FlagDefinition> = vec![
            (
                'q',
                "quiet".into(),
                "silences the output".into(),
                Value::Bool(false),
            ),
            (
                'c',
                "config".into(),
                "supply config file".into(),
                Value::List(Vec::new()),
            ),
            (
                'h',
                "help".into(),
                "get some help".into(),
                Value::Bool(false),
            ),
            (
                'v',
                "verbose".into(),
                "level of verbosity".into(),
                Value::Bool(false),
            ),
            (
                'r',
                "rev".into(),
                "revision hash".into(),
                Value::Str("".to_string()),
            ),
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
        let definition = (
            'q',
            "quiet".into(),
            "silences the output".into(),
            Value::Bool(false),
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();

        let args = vec!["-q"];

        let _ = parser
            .parse_single_hyphen_flag(&mut args.into_iter().enumerate().peekable(), &mut opts)
            .unwrap();
        let quiet: bool = opts.get("quiet").unwrap().clone().try_into().unwrap();
        assert!(quiet);
    }

    #[test]
    fn test_parse_single_value_flag() {
        let definition = (
            'c',
            "config".into(),
            "supply config file".into(),
            Value::Str("".to_string()),
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();
        const PATH: &str = "$HOME/path/to/config/file";

        let args = vec!["-c", PATH];

        let _result = parser
            .parse_single_hyphen_flag(&mut args.into_iter().enumerate().peekable(), &mut opts);
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
            .parse_single_hyphen_flag(
                &mut clustered_args.into_iter().enumerate().peekable(),
                &mut opts,
            )
            .unwrap();

        //assert_eq!(v.len(), CLUSTER.len() - 1);
    }

    #[test]
    fn test_parse_long_single_no_value() {
        let definition = (
            'q',
            "quiet".into(),
            "silences the output".into(),
            Value::Bool(false),
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();

        let args = vec!["--quiet"];

        let _ = parser
            .parse_double_hyphen_flag(&mut args.into_iter().enumerate().peekable(), &mut opts)
            .unwrap();

        //assert_eq!(parsed_flag, flag.long_name);
    }

    #[test]
    fn test_parse_long_single_with_value() {
        let definition = (
            'c',
            "config".into(),
            "supply config file".into(),
            Value::Str("".to_string()),
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();
        const PATH: &str = "$HOME/path/to/config/file";

        let args = vec!["--config", PATH];

        let _ = parser
            .parse_double_hyphen_flag(&mut args.into_iter().enumerate().peekable(), &mut opts)
            .unwrap();

        //assert_eq!(parsed_flag, flag.long_name);
        //let s: String = value.clone().try_into().unwrap();
        //assert_eq!(s, PATH.to_string());
    }

    #[test]
    fn test_parse_long_single_int_value() {
        let definition = (
            'n',
            "number".into(),
            "supply a number".into(),
            Value::Int(0),
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);
        let mut opts = parser.opts.clone();

        let args = vec!["--number", "60"];

        let _ = parser
            .parse_double_hyphen_flag(&mut args.into_iter().enumerate().peekable(), &mut opts)
            .unwrap();

        //assert_eq!(parsed_flag, flag.long_name);
        //let i: i64 = value.clone().try_into().unwrap();
        //assert_eq!(i, 60);
    }

    #[test]
    fn test_parse_long_single_list_value() {
        let definition = (
            'n',
            "number".into(),
            "supply a list of numbers".into(),
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
            "foo".to_string(),
            "--number".to_string(),
            "3".to_string(),
            "bar".to_string(),
        ];

        let result = parser.parse_args(&args).unwrap();

        assert_eq!(result.first_arg_index(), 4);

        let list: Vec<String> = result.get("number").unwrap().clone().try_into().unwrap();

        assert_eq!(list, vec!["60", "59", "3"]);
    }

    #[test]
    fn test_parse_long_and_short_single_list_value() {
        let definition = (
            'n',
            "number".into(),
            "supply a list of numbers".into(),
            Value::List(Vec::new()),
        );
        let flag = Flag::new(&definition);
        let flags = vec![flag.clone()];
        let parser = Parser::new(&flags);

        let args = create_args(vec![
            "--number", "60", "--number", "59", "-n", "3", "-n5", "foo", "bar",
        ]);

        let result = parser.parse_args(&args).unwrap();

        assert_eq!(result.first_arg_index(), 7);

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
            "config".into(),
            "supply a config file".into(),
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
            "config".into(),
            "supply multiple config files".into(),
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
            "config".into(),
            "supply multiple config files".into(),
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

        assert_eq!(result.first_arg_index(), 0);

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
            "template".into(),
            "specify a template".into(),
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
    #[should_panic]
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
    #[should_panic]
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
    #[should_panic]
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
    #[should_panic]
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

    #[test]
    fn test_prefix_match_double_flag() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--con", "test"]);

        let result = parser.parse_args(&args).unwrap();

        let configs: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs.get(0).unwrap(), "test");
    }

    #[test]
    fn test_prefix_match_trailing_equals() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--con="]);

        let result = parser.parse_args(&args).unwrap();

        let configs: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs.get(0).unwrap(), "");
    }

    #[test]
    fn test_prefix_match_ambiguous() {
        let definitions = vec![
            (
                'c',
                "config".into(),
                "config overrides".into(),
                Value::List(Vec::new()),
            ),
            (
                ' ',
                "configfile".into(),
                "config files".into(),
                Value::List(Vec::new()),
            ),
        ];
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--co="]); // this is an ambiguous flag

        let result = parser.parse_args(&args).unwrap();

        let configs: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();
        let configfiles: Vec<String> = result
            .get("configfile")
            .unwrap()
            .clone()
            .try_into()
            .unwrap();
        assert_eq!(configs.len(), 0);
        assert_eq!(configfiles.len(), 0);
    }

    #[test]
    fn test_prefix_match_mixed_with_exact_match_and_short_flags() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec![
            "--c=",
            "--config",
            "section.key=val",
            "-c=",
            "--conf=section.key=val",
        ]);

        let result = parser.parse_args(&args).unwrap();

        assert_eq!(result.first_arg_index(), 5);

        let configs: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();

        let expected = vec!["", "section.key=val", "=", "section.key=val"];

        assert_eq!(configs, expected);
    }

    #[test]
    fn test_no_prefix_match() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parsing_options = ParseOptions::new().ignore_prefix(true);
        let parser = Parser::new(&flags).with_parsing_options(parsing_options);

        let args = create_args(vec!["--conf", "section.key=val"]);

        let result = parser.parse_args(&args).unwrap();

        let configs: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();

        assert_eq!(configs.len(), 0);
    }

    #[test]
    fn test_no_errors_match() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parsing_options = ParseOptions::new().ignore_prefix(true).ignore_errors(true);
        let parser = Parser::new(&flags).with_parsing_options(parsing_options);

        let args = create_args(vec!["--shallow", "--config", "section.key=val"]);

        let result = parser.parse_args(&args).unwrap();

        let configs: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();

        assert_eq!(configs, vec!["section.key=val"]);
    }

    #[test]
    fn test_aliased_option() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parsing_options = ParseOptions::new()
            .flag_alias("conf", "config")
            .ignore_prefix(true);
        let parser = Parser::new(&flags).with_parsing_options(parsing_options);

        let args = create_args(vec!["--shallow", "--conf", "section.key=val"]);

        let result = parser.parse_args(&args).unwrap();

        let configs: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();

        assert_eq!(configs, vec!["section.key=val"]);
    }

    #[test]
    fn test_early_parse() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parsing_options = ParseOptions::new().early_parse(true).ignore_prefix(true);
        let parser = Parser::new(&flags).with_parsing_options(parsing_options);

        let args = create_args(vec!["-qc."]);

        let result = parser.parse_args(&args).unwrap();

        let configs: Vec<String> = result.get("config").unwrap().clone().try_into().unwrap();

        assert_eq!(configs.len(), 0);
    }

    #[test]
    fn test_keep_sep() {
        let definitions = definitions();
        let flags = Flag::from_flags(&definitions);
        let parsing_options = ParseOptions::new()
            .early_parse(true)
            .ignore_prefix(true)
            .keep_sep(true);
        let parser = Parser::new(&flags).with_parsing_options(parsing_options);

        let args = create_args(vec!["--", "-1", "4"]);

        let result = parser.parse_args(&args).unwrap();

        assert_eq!(result.first_arg_index(), 0);

        let parsed_args = result.args().clone();

        assert_eq!(parsed_args, vec!["--", "-1", "4"]);
    }

    #[test]
    fn test_parse_flag_starting_with_no_with_positive_arg() {
        let definitions = vec![(
            ' ',
            "no-commit".into(),
            "leaves the changes in the working copy".into(),
            Value::Bool(false),
        )];
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--commit"]);

        let result = parser.parse_args(&args).unwrap();

        if let Value::Bool(no_commit) = result.get("no-commit").unwrap() {
            assert!(!no_commit);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_parse_flag_starting_with_no_with_negative_arg() {
        let definitions = vec![(
            ' ',
            "no-commit".into(),
            "leaves the changes in the working copy".into(),
            Value::Bool(false),
        )];
        let flags = Flag::from_flags(&definitions);
        let parser = Parser::new(&flags);

        let args = create_args(vec!["--no-commit"]);

        let result = parser.parse_args(&args).unwrap();

        if let Value::Bool(no_commit) = result.get("no-commit").unwrap() {
            assert!(no_commit);
        } else {
            assert!(false);
        }
    }

}
