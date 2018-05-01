// Copyright 2004-present Facebook. All Rights Reserved.

//! Parse command line arguments.
//! Why not use clap?  clap doesn't follow the way that the mercurial
//! CLI parses its arguments, and doesn't make it easy to deal with
//! the sampled command evolving its arguments over time.
//! You probably don't want to try to use this code outside of the context
//! of this telemetry utility.  In the future we will likely expand this
//! to also handle alias expansion.

/// Define an argument to a command.
/// The argument can have --long or -s short forms and may
/// require a value.
#[derive(Clone)]
pub struct Arg {
    /// The name of the argument.  This is the name that must match
    /// when recalling arguments via the ParsedArgs interface.
    name: String,
    /// The set of long option aliases that refer to this Arg.
    /// These are stored with the -- prefix included.
    long: Vec<String>,
    /// The short option alias referring to this arg.
    /// Stored with the - prefix included.
    short: Option<String>,
    /// If true we know to look ahead for the argument value.
    requires_value: bool,
}

#[derive(Copy, Clone)]
pub enum HelpVisibility {
    Always,
    VerboseOnly,
}

/// Define a command or subcommand.
/// A command may have a list of arguments and, for subcommands,
/// be aliased to alternative names.
pub struct Command {
    /// The list of known arguments
    args: Vec<Arg>,
    /// The list of aliases for this subcommand.  The 0th
    /// element is considered to be the canonical name.
    aliases: Vec<String>,
    /// The known subcommands of this command
    subcommands: Vec<Command>,
    /// boring commands are intended not to be logged
    boring: bool,
    // We hide some commands in non-verbose help output
    help_visibility: HelpVisibility,
}

/// Holds the result of a successfully recognized argument
#[derive(Debug, PartialEq)]
pub struct ParsedArgument {
    /// Corresponds to Arg::name for the matched argument
    pub name: String,
    /// Holds the extracted argument value if any. This may
    /// be populated even if Arg::requires_value == false
    /// if the parsed option is of the form "--foo=bar".
    /// This may be none even if Arg::requires_value == true
    /// if the option is of the form "--foo" and it was the
    /// last element in the arguments slice.
    pub value: Option<String>,
}

/// Holds the result of parsing an argument list.
#[derive(Default, Debug, PartialEq)]
pub struct ParsedArgs {
    /// Corresponds to Command::aliases[0] for the parsed Command
    pub name: String,
    /// Holds the list of recognized arguments
    pub known_args: Vec<ParsedArgument>,
    /// Holds the list of unrecognized arguments (only those starting with -)
    pub unknown_args: Vec<String>,
    /// Holds the list of unrecognized positional arguments (!starting with -)
    pub positional: Vec<String>,
    /// If we recognized a subcommand, the parsed results from that command.
    pub subcommand: Option<Box<ParsedArgs>>,
}

impl ParsedArgs {
    /// Returns the singular value associated with a defined argument.
    /// This is the most recent value that we observed for that argument.
    pub fn value_of(&self, name: &str) -> Option<&String> {
        self.all_values_of(name)
            .iter()
            .filter_map(|opt_item| *opt_item)
            .rev()
            .nth(0)
    }

    /// Returns true if the named argument was recognized and recorded.
    /// It does not guarantee that value_of().is_some() == true.
    pub fn is_present(&self, name: &str) -> bool {
        self.all_values_of(name).iter().any(|_| true)
    }

    /// A helper function for resolving arguments.
    /// The current implementation propagates the arguments defined
    /// on a parent command down to any subcommands that get defined.
    /// As we walk into subcommands we will accumulate arguments in
    /// the most recently observed ParsedArgs on the stack.
    /// In order to correctly resolve all possible arguments we therefore
    /// need to walk over the stack of ParsedArgs.
    /// This function returns that list.
    fn all_parsed_args(&self) -> Vec<&ParsedArgs> {
        let mut args = Vec::new();
        args.push(self);
        let mut child = &self.subcommand;
        loop {
            if let &Some(ref subcommand) = child {
                args.push(&*subcommand);
                child = &subcommand.subcommand;
            } else {
                break;
            }
        }
        args
    }

    /// Returns the optional value for each occurrence of the named argument
    /// in the order that they were specified in the argument slice.
    /// An entry may be populated even if Arg::requires_value == false
    /// if the parsed option is of the form "--foo=bar".
    /// An entry may be none even if Arg::requires_value == true
    /// if the option is of the form "--foo" and it was the
    /// last element in the arguments slice.
    pub fn all_values_of(&self, name: &str) -> Vec<Option<&String>> {
        self.all_parsed_args()
            .iter()
            .flat_map(|result| {
                result.known_args.iter().filter_map(|arg| {
                    if arg.name == name {
                        Some(arg.value.as_ref())
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Returns an optional reference to the parsed information for the
    /// specified subcommand.  If that subcommand wasn't recognized,
    /// returns None.
    pub fn subcommand_matches(&self, name: &str) -> Option<&ParsedArgs> {
        match self.subcommand {
            Some(ref sub) if sub.name == name => Some(&*sub),
            _ => None,
        }
    }
}

impl Arg {
    /// Define a new named argument.
    /// It is initialized to include a --name long argument.
    pub fn with_name(name: &str) -> Arg {
        Arg {
            name: name.into(),
            long: vec![format!("--{}", name)],
            short: None,
            requires_value: false,
        }
    }

    /// Mark this argument as requiring a value.
    pub fn requires_value(mut self) -> Self {
        self.requires_value = true;
        self
    }

    /// Set the short option flag to the specified string.
    pub fn short(mut self, short: u8) -> Self {
        self.short = Some(format!("-{}", short as char));
        self
    }

    /// Add a long option alias to this argument
    pub fn long(mut self, alias: &str) -> Self {
        self.long.push(format!("--{}", alias));
        self
    }
}

impl Command {
    /// Define a new named command
    pub fn with_name(name: &str) -> Command {
        Command {
            aliases: vec![name.into()],
            args: Vec::new(),
            subcommands: Vec::new(),
            boring: false,
            help_visibility: HelpVisibility::VerboseOnly,
        }
    }

    /// Add an argument to the list of known arguments
    pub fn arg(mut self, arg: Arg) -> Self {
        self.args.push(arg);
        self
    }

    /// Add an alias for the subcommand
    pub fn alias(mut self, alias: &str) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Add a subcommand to the list of possible subcommands
    pub fn subcommand(mut self, mut cmd: Command) -> Self {
        // For the sake of easier parsing later, copy the arguments
        // from the parent command into the child command.  This allows
        // global arguments to be visible in the subcommand.
        cmd.args.extend_from_slice(&self.args[..]);
        self.subcommands.push(cmd);
        self
    }

    /// Set the boring flag for this command.
    /// Boring commands will not be logged.
    pub fn boring(mut self) -> Self {
        self.boring = true;
        self
    }

    pub fn help_visibility(mut self, value: HelpVisibility) -> Self {
        self.help_visibility = value;
        self
    }

    /// Given an argument slice, parse it and return the parsed state.
    pub fn parse(&self, arguments: &[String]) -> ParsedArgs {
        let mut parsed = ParsedArgs::default();
        parsed.name = self.aliases[0].clone();
        let mut skip_next = false;

        for (n, arg) in arguments.iter().enumerate() {
            if skip_next {
                skip_next = false;
                continue;
            }

            if arg == "--" {
                // Stop parsing here; all remaining args are positional
                parsed.positional.extend_from_slice(&arguments[n..]);
                break;
            }

            if arg.starts_with("--") {
                // We can have either ["--foo", "bar"]
                //                    ["--foo=bar"]
                //                    ["--foo"] (!requires_value)
                let switch_pieces: Vec<_> = arg.splitn(2, '=').collect();
                let switch_name = &switch_pieces[0];

                // We select the best matching argument definition.
                // The best is the shortest of those that have a matching prefix.
                let mut best = None;
                for argdef in self.args.iter() {
                    for long in argdef.long.iter() {
                        if long.starts_with(switch_name) {
                            match best {
                                None => {
                                    best = Some((long.len(), argdef));
                                }
                                Some((len, _)) if long.len() < len => {
                                    best = Some((long.len(), argdef));
                                }
                                _ => {}
                            }
                        }
                    }
                }

                if let Some((_, argdef)) = best {
                    let value: Option<String> = if switch_pieces.len() == 2 {
                        // Take the value from the RHS of the = sign
                        switch_pieces.get(1).map(|x| (*x).into())
                    } else if argdef.requires_value {
                        // Take the next argument from the list,
                        // and remember to skip that on the next
                        // loop iteration.
                        skip_next = true;
                        arguments.get(n + 1).map(|x| x.clone())
                    } else {
                        // Not known to accept an argument.
                        None
                    };

                    parsed.known_args.push(ParsedArgument {
                        name: argdef.name.clone(),
                        value: value,
                    });
                } else {
                    // ambiguous or no match
                    parsed.unknown_args.push(arg.clone());
                }
                continue;
            }

            if arg.starts_with("-") {
                if arg == "-" {
                    // TODO: There is not a great way to declare an Arg of this form today.
                    parsed.unknown_args.push(arg.clone());
                    continue;
                }

                enum Details {
                    Known(Option<String>),
                    Unknown,
                }

                struct ArgToRecord<'a> {
                    name: &'a str,
                    details: Details,
                }

                // Because `parsed` cannot be borrowed mutably for both parsed.known_args() and
                // parsed.unknown_args(), we create a single closure that handles both cases.
                let mut record_arg = |arg| match arg {
                    ArgToRecord {
                        name,
                        details: Details::Known(value),
                    } => parsed.known_args.push(ParsedArgument {
                        name: name.to_string(),
                        value,
                    }),
                    ArgToRecord {
                        name,
                        details: Details::Unknown,
                    } => parsed.unknown_args.push(name.to_string()),
                };

                // We select the best matching short argument definition.
                // This is simply the first short option that matches.
                // Since short options are intended to be single character
                // flags there can be no ambiguity.
                if let Some(argdef) = self.find_short_argdef(arg.as_bytes()[1]) {
                    if argdef.requires_value {
                        if arg.len() > 2 {
                            // -R=foo -> value = "foo"
                            // -Rfoo -> value = "foo"
                            let bytes = &arg.as_bytes()[2..];
                            let value = if bytes[0] == b'=' {
                                String::from_utf8(bytes[1..].to_vec()).ok()
                            } else {
                                String::from_utf8(bytes.to_vec()).ok()
                            };
                            record_arg(ArgToRecord {
                                name: &argdef.name,
                                details: Details::Known(value),
                            });
                        } else {
                            // Take the next argument from the list,
                            // and remember to skip that on the next
                            // loop iteration.
                            skip_next = true;
                            let value = arguments.get(n + 1).map(|x| x.clone());
                            record_arg(ArgToRecord {
                                name: &argdef.name,
                                details: Details::Known(value),
                            });
                        }
                    } else if arg.len() == 2 {
                        record_arg(ArgToRecord {
                            name: &argdef.name,
                            details: Details::Known(None),
                        });
                    } else {
                        // Not known to accept an argument. This could be something like
                        // `hg status -mardui` where a number of boolean arguments are specified
                        // at once.
                        //
                        // Our strategy is to check every character in the arg and see if it is a
                        // valid short name for a boolean arg. If so, we set all of the
                        // corresponding args to "true". If not, we record arg as an unknown arg.
                        let bool_argdefs: Vec<ArgToRecord> = arg.bytes()
                            .skip(1)
                            .filter_map(|arg_char| match self.find_short_argdef(arg_char) {
                                Some(ref argdef) if !argdef.requires_value => Some(ArgToRecord {
                                    name: &argdef.name,
                                    details: Details::Known(None),
                                }),
                                Some(_) => None,
                                None => None,
                            })
                            .collect();

                        if bool_argdefs.len() == arg.len() - 1 {
                            // All of the args were bool args.
                            for arg in bool_argdefs {
                                record_arg(arg);
                            }
                        } else {
                            record_arg(ArgToRecord {
                                name: arg,
                                details: Details::Unknown,
                            });
                        }
                    }
                } else {
                    // ambiguous or no match
                    record_arg(ArgToRecord {
                        name: arg,
                        details: Details::Unknown,
                    });
                }
                continue;
            }

            // Not a switch at all; perhaps it is a subcommand?
            if parsed.subcommand.is_none() && parsed.positional.len() == 0 && arg.len() > 0 {
                // We select the best matching subcommand definition.
                // The best is the shortest of those that have a matching prefix.
                let mut best = None;
                for sub in self.subcommands.iter() {
                    for alias in sub.aliases.iter() {
                        if alias.starts_with(arg) {
                            match best {
                                None => {
                                    best = Some((alias.len(), sub));
                                }
                                Some((len, _)) if alias.len() < len => {
                                    best = Some((alias.len(), sub));
                                }
                                _ => {}
                            }
                        }
                    }
                }

                if let Some((_, sub)) = best {
                    parsed.subcommand = Some(Box::new(sub.parse(&arguments[n + 1..])));
                    // Parsing the subcommand consumes all remaining args,
                    // so we stop looping here.
                    break;
                }

                // Either no candidates, or have ambiguous candidates
            }

            // Lump it in with the positional arguments
            parsed.positional.push(arg.clone());
        }

        parsed
    }

    /// Given arg_char, try to find an Arg whose short version uses that single character.
    fn find_short_argdef(&self, arg_char: u8) -> Option<&Arg> {
        for arg in self.args.iter() {
            if let Some(ref value) = arg.short {
                if value.as_bytes()[1] == arg_char {
                    return Some(&arg);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn arglist(args: &[&str]) -> Vec<String> {
        args.iter().map(|x| (*x).into()).collect()
    }

    #[test]
    fn basic_args() {
        let cmd = Command::with_name("frob").arg(Arg::with_name("help").short(b'h'));

        let p = cmd.parse(&arglist(&["--help"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.name, "frob");
        assert_eq!(p.is_present("help"), true);
        assert!(p.value_of("help").is_none());

        // Short and long are both recognized
        let p = cmd.parse(&arglist(&["--help", "-h"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("help"), true);
        assert_eq!(p.all_values_of("help").len(), 2);

        // Short option on its own is recognized
        let p = cmd.parse(&arglist(&["-h"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("help"), true);

        // Relative order of positional and switches doesn't matter
        let p = cmd.parse(&arglist(&["--help", "hello"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("help"), true);
        assert_eq!(p.positional, arglist(&["hello"]));

        let p = cmd.parse(&arglist(&["hello", "--help"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("help"), true);
        assert_eq!(p.positional, arglist(&["hello"]));

        // Check that we can match a prefix
        let p = cmd.parse(&arglist(&["--hel"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("help"), true);

        let p = cmd.parse(&arglist(&["--he"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("help"), true);

        let p = cmd.parse(&arglist(&["--h"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("help"), true);
    }

    #[test]
    fn switches_with_values() {
        let cmd = Command::with_name("frob").arg(Arg::with_name("config").requires_value());

        let p = cmd.parse(&arglist(&["--config"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("config"), true);
        assert!(p.value_of("config").is_none());

        let p = cmd.parse(&arglist(&["--config", "foo"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("config"), true);
        assert_eq!(p.value_of("config").unwrap(), "foo");

        // value_of returns last specified value
        let p = cmd.parse(&arglist(&["--config", "foo", "--config", "bar"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("config"), true);
        assert_eq!(p.value_of("config").unwrap(), "bar");
        // all_values_of holds all the expected values
        let all_values = p.all_values_of("config");
        assert_eq!(all_values[0].unwrap(), "foo");
        assert_eq!(all_values[1].unwrap(), "bar");
        assert_eq!(all_values.len(), 2);

        // Incomplete final arg
        let p = cmd.parse(&arglist(&["--config", "foo", "--config"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("config"), true);
        // We see only the complete first arg in value_of()
        assert_eq!(p.value_of("config").unwrap(), "foo");
        // all_values_of holds all the expected values
        let all_values = p.all_values_of("config");
        assert_eq!(all_values[0].unwrap(), "foo");
        assert!(all_values[1].is_none());
        assert_eq!(all_values.len(), 2);
    }

    #[test]
    fn switch_with_alias() {
        let cmd =
            Command::with_name("frob").arg(Arg::with_name("config").long("set").requires_value());

        let p = cmd.parse(&arglist(&["--config", "foo", "--set", "bar"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("config"), true);
        assert_eq!(p.value_of("config").unwrap(), "bar");
        // all_values_of holds all the expected values
        let all_values = p.all_values_of("config");
        assert_eq!(all_values[0].unwrap(), "foo");
        assert_eq!(all_values[1].unwrap(), "bar");
        assert_eq!(all_values.len(), 2);
    }

    #[test]
    fn ambiguous_switches() {
        let cmd = Command::with_name("frob")
            .arg(Arg::with_name("encoding").requires_value())
            .arg(Arg::with_name("encodingmode").requires_value());

        let p = cmd.parse(&arglist(&["--encoding", "A", "--encodingmode", "B"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("encoding"), true);
        assert_eq!(p.value_of("encoding").unwrap(), "A");
        assert_eq!(p.is_present("encodingmode"), true);
        assert_eq!(p.value_of("encodingmode").unwrap(), "B");

        let p = cmd.parse(&arglist(&["--encodingm", "B"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("encoding"), false);
        assert_eq!(p.is_present("encodingmode"), true);
        assert_eq!(p.value_of("encodingmode").unwrap(), "B");

        let p = cmd.parse(&arglist(&["--encodin", "A"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("encodingmode"), false);
        assert_eq!(p.is_present("encoding"), true);
        assert_eq!(p.value_of("encoding").unwrap(), "A");

        // Name is longer than any valid switch
        let p = cmd.parse(&arglist(&["--encodingmodel", "B"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("encoding"), false);
        assert_eq!(p.is_present("encodingmode"), false);
        assert_eq!(p.unknown_args, arglist(&["--encodingmodel"]));
        assert_eq!(p.positional, arglist(&["B"]));
    }

    #[test]
    fn multiple_boolean_switches_in_single_arg() {
        let cmd = Command::with_name("status")
            .arg(Arg::with_name("modified").short(b'm'))
            .arg(Arg::with_name("added").short(b'a'))
            .arg(Arg::with_name("removed").short(b'r'))
            .arg(Arg::with_name("deleted").short(b'd'))
            .arg(Arg::with_name("clean").short(b'c'))
            .arg(Arg::with_name("unknown").short(b'u'))
            .arg(Arg::with_name("ignored").short(b'i'));

        let p = cmd.parse(&arglist(&["-mardu"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("modified"), true);
        assert_eq!(p.is_present("added"), true);
        assert_eq!(p.is_present("removed"), true);
        assert_eq!(p.is_present("deleted"), true);
        assert_eq!(p.is_present("clean"), false);
        assert_eq!(p.is_present("unknown"), true);
        assert_eq!(p.is_present("ignored"), false);

        // One unrecognized switch (z) ruins it for everyone.
        let p = cmd.parse(&arglist(&["-marduz"]));
        assert_eq!(p.unknown_args, arglist(&["-marduz"]));
        assert_eq!(p.is_present("modified"), false);
        assert_eq!(p.is_present("added"), false);
        assert_eq!(p.is_present("removed"), false);
        assert_eq!(p.is_present("deleted"), false);
        assert_eq!(p.is_present("clean"), false);
        assert_eq!(p.is_present("unknown"), false);
        assert_eq!(p.is_present("ignored"), false);
    }

    #[test]
    fn subcommands_inherit_top_args() {
        let cmd = Command::with_name("frob")
            .arg(Arg::with_name("config").requires_value())
            .subcommand(Command::with_name("foo"))
            .subcommand(Command::with_name("bar"));

        let p = cmd.parse(&arglist(&["--config", "foo"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("config"), true);
        assert_eq!(p.value_of("config").unwrap(), "foo");

        let p = cmd.parse(&arglist(&["--config", "foo", "foo"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("config"), true);
        assert_eq!(p.value_of("config").unwrap(), "foo");
        assert!(p.subcommand_matches("foo").is_some());

        let p = cmd.parse(&arglist(&["--config", "foo", "foo", "--config", "bar"]));
        println!("parsed: {:?}", p);
        assert_eq!(p.is_present("config"), true);
        assert_eq!(p.value_of("config").unwrap(), "bar");
        assert!(p.subcommand_matches("foo").is_some());
    }

}
