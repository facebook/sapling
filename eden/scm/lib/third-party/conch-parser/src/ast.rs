//! Defines abstract representations of the shell source.
use std::rc::Rc;
use std::sync::Arc;
use std::{fmt, ops};
use serde::Serialize;

pub mod builder;

/// Type alias for the default `Parameter` representation.
pub type DefaultParameter = Parameter<String>;

/// Represents reading a parameter (or variable) value, e.g. `$foo`.
///
/// Generic over the representation of variable names.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum Parameter<T> {
    /// $@
    At,
    /// $*
    #[serde(rename="StarParameter")]
    Star,
    /// $#
    Pound,
    /// $?
    #[serde(rename="QuestionParameter")]
    Question,
    /// $-
    Dash,
    /// $$
    Dollar,
    /// $!
    Bang,
    /// $0, $1, ..., $9, ${100}
    Positional(u32),
    /// $foo
    #[serde(rename="VarParameter")]
    Var(T),
}

/// Type alias for the default `ParameterSubstitution` representation.
pub type DefaultParameterSubstitution = ParameterSubstitution<
    DefaultParameter,
    TopLevelWord<String>,
    TopLevelCommand<String>,
    DefaultArithmetic,
>;

/// A parameter substitution, e.g. `${param-word}`.
///
/// Generic over the representations of parameters, shell words and
/// commands, and arithmetic expansions.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum ParameterSubstitution<P, W, C, A> {
    /// Returns the standard output of running a command, e.g. `$(cmd)`
    Command(Vec<C>),
    /// Returns the length of the value of a parameter, e.g. `${#param}`
    Len(P),
    /// Returns the resulting value of an arithmetic subsitution, e.g. `$(( x++ ))`
    Arith(Option<A>),
    /// Use a provided value if the parameter is null or unset, e.g.
    /// `${param:-[word]}`.
    /// The boolean indicates the presence of a `:`, and that if the parameter has
    /// a null value, that situation should be treated as if the parameter is unset.
    Default(bool, P, Option<W>),
    /// Assign a provided value to the parameter if it is null or unset,
    /// e.g. `${param:=[word]}`.
    /// The boolean indicates the presence of a `:`, and that if the parameter has
    /// a null value, that situation should be treated as if the parameter is unset.
    #[serde(rename="AssignSubstitution")]
    Assign(bool, P, Option<W>),
    /// If the parameter is null or unset, an error should result with the provided
    /// message, e.g. `${param:?[word]}`.
    /// The boolean indicates the presence of a `:`, and that if the parameter has
    /// a null value, that situation should be treated as if the parameter is unset.
    Error(bool, P, Option<W>),
    /// If the parameter is NOT null or unset, a provided word will be used,
    /// e.g. `${param:+[word]}`.
    /// The boolean indicates the presence of a `:`, and that if the parameter has
    /// a null value, that situation should be treated as if the parameter is unset.
    Alternative(bool, P, Option<W>),
    /// Remove smallest suffix pattern from a parameter's value, e.g. `${param%pattern}`
    RemoveSmallestSuffix(P, Option<W>),
    /// Remove largest suffix pattern from a parameter's value, e.g. `${param%%pattern}`
    RemoveLargestSuffix(P, Option<W>),
    /// Remove smallest prefix pattern from a parameter's value, e.g. `${param#pattern}`
    RemoveSmallestPrefix(P, Option<W>),
    /// Remove largest prefix pattern from a parameter's value, e.g. `${param##pattern}`
    RemoveLargestPrefix(P, Option<W>),
}

/// A type alias for the default hiearchy for representing shell words.
pub type ShellWord<T, W, C> = ComplexWord<
    Word<
        T,
        SimpleWord<T, Parameter<T>, Box<ParameterSubstitution<Parameter<T>, W, C, Arithmetic<T>>>>,
    >,
>;

/// Type alias for the default `ComplexWord` representation.
pub type DefaultComplexWord = ComplexWord<DefaultWord>;

/// Represents whitespace delimited text.
///
/// Generic over the representation of a whitespace delimited word.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum ComplexWord<W> {
    /// Several distinct words concatenated together.
    Concat(Vec<W>),
    /// A regular word.
    #[serde(rename="SingleWord")]
    Single(W),
}

/// Type alias for the default `Word` representation.
pub type DefaultWord = Word<String, DefaultSimpleWord>;

/// Represents whitespace delimited single, double, or non quoted text.
///
/// Generic over the representation of single-quoted literals, and non-quoted words.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum Word<L, W> {
    /// A regular word.
    #[serde(rename="SimpleWord")]
    Simple(W),
    /// List of words concatenated within double quotes.
    DoubleQuoted(Vec<W>),
    /// List of words concatenated within single quotes. Virtually
    /// identical as a literal, but makes a distinction between the two.
    SingleQuoted(L),
}

/// Type alias for the default `SimpleWord` representation.
pub type DefaultSimpleWord =
    SimpleWord<String, DefaultParameter, Box<DefaultParameterSubstitution>>;

/// Represents the smallest fragment of any text.
///
/// Generic over the representation of a literals, parameters, and substitutions.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum SimpleWord<L, P, S> {
    /// A non-special literal word.
    #[serde(rename="LiteralWord")]
    Literal(L),
    /// A token which normally has a special meaning is treated as a literal
    /// because it was escaped, typically with a backslash, e.g. `\"`.
    Escaped(L),
    /// Access of a value inside a parameter, e.g. `$foo` or `$$`.
    Param(P),
    /// A parameter substitution, e.g. `${param-word}`.
    Subst(S),
    /// Represents `*`, useful for handling pattern expansions.
    #[serde(rename="StarWord")]
    Star,
    /// Represents `?`, useful for handling pattern expansions.
    #[serde(rename="QuestionWord")]
    Question,
    /// Represents `[`, useful for handling pattern expansions.
    SquareOpen,
    /// Represents `]`, useful for handling pattern expansions.
    SquareClose,
    /// Represents `~`, useful for handling tilde expansions.
    Tilde,
    /// Represents `:`, useful for handling tilde expansions.
    Colon,
}

/// Type alias for the default `Redirect` representation.
pub type DefaultRedirect = Redirect<TopLevelWord<String>>;

/// Represents redirecting a command's file descriptors.
///
/// Generic over the representation of a shell word.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum Redirect<W> {
    /// Open a file for reading, e.g. `[n]< file`.
    Read(Option<u16>, W),
    /// Open a file for writing after truncating, e.g. `[n]> file`.
    Write(Option<u16>, W),
    /// Open a file for reading and writing, e.g. `[n]<> file`.
    ReadWrite(Option<u16>, W),
    /// Open a file for writing, appending to the end, e.g. `[n]>> file`.
    Append(Option<u16>, W),
    /// Open a file for writing, failing if the `noclobber` shell option is set, e.g. `[n]>| file`.
    Clobber(Option<u16>, W),
    /// Lines contained in the source that should be provided by as input to a file descriptor.
    Heredoc(Option<u16>, W),
    /// Duplicate a file descriptor for reading, e.g. `[n]<& [n|-]`.
    DupRead(Option<u16>, W),
    /// Duplicate a file descriptor for writing, e.g. `[n]>& [n|-]`.
    DupWrite(Option<u16>, W),
}

/// A grouping of guard and body commands.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
pub struct GuardBodyPair<C> {
    /// The guard commands, which if successful, should lead to the
    /// execution of the body commands.
    pub guard: Vec<C>,
    /// The body commands to execute if the guard is successful.
    pub body: Vec<C>,
}

/// A grouping of patterns and body commands.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
pub struct PatternBodyPair<W, C> {
    /// Pattern alternatives to match against.
    pub patterns: Vec<W>,
    /// The body commands to execute if the pattern matches.
    pub body: Vec<C>,
}

/// Type alias for the default `Command` representation.
pub type DefaultCommand = Command<DefaultAndOrList>;

/// Represents any valid shell command.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum Command<T> {
    /// A command that runs asynchronously, that is, the shell will not wait
    /// for it to exit before running the next command, e.g. `foo &`.
    Job(T),
    /// A list of and/or commands, e.g. `foo && bar || baz`.
    List(T),
}

/// A type alias over an and/or list of conventional shell commands.
///
/// Generic over the representation of literals, shell words, commands, and redirects.
/// Uses `Rc` wrappers around function declarations.
pub type CommandList<T, W, C> = AndOrList<ListableCommand<ShellPipeableCommand<T, W, C>>>;

/// A type alias over an and/or list of conventional shell commands.
///
/// Generic over the representation of literals, shell words, commands, and redirects.
/// Uses `Arc` wrappers around function declarations.
pub type AtomicCommandList<T, W, C> =
    AndOrList<ListableCommand<AtomicShellPipeableCommand<T, W, C>>>;

/// A type alias for the default hiearchy to represent pipeable commands,
/// using `Rc` wrappers around function declarations.
pub type ShellPipeableCommand<T, W, C> = PipeableCommand<
    T,
    Box<SimpleCommand<T, W, Redirect<W>>>,
    Box<ShellCompoundCommand<T, W, C>>,
    Rc<ShellCompoundCommand<T, W, C>>,
>;

/// A type alias for the default hiearchy to represent pipeable commands,
/// using `Arc` wrappers around function declarations.
pub type AtomicShellPipeableCommand<T, W, C> = PipeableCommand<
    T,
    Box<SimpleCommand<T, W, Redirect<W>>>,
    Box<ShellCompoundCommand<T, W, C>>,
    Arc<ShellCompoundCommand<T, W, C>>,
>;

/// A command which conditionally runs based on the exit status of the previous command.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum AndOr<T> {
    /// A compound command which should run only if the previously run command succeeded.
    And(T),
    /// A compound command which should run only if the previously run command failed.
    Or(T),
}

/// Type alias for the default `AndOrList` representation.
pub type DefaultAndOrList = AndOrList<DefaultListableCommand>;

/// A nonempty list of `AndOr` commands, e.g. `foo && bar || baz`.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
pub struct AndOrList<T> {
    /// The first command that always runs.
    pub first: T,
    /// The remainder of the conditional commands which may or may not run.
    pub rest: Vec<AndOr<T>>,
}

/// Type alias for the default `ListableCommand` representation.
pub type DefaultListableCommand = ListableCommand<DefaultPipeableCommand>;

/// Commands that can be used within an and/or list.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum ListableCommand<T> {
    /// A chain of concurrent commands where the standard output of the
    /// previous becomes the standard input of the next, e.g.
    /// `[!] foo | bar | baz`.
    ///
    /// The bool indicates if a logical negation of the last command's status
    /// should be returned.
    Pipe(bool, Vec<T>),
    /// A single command not part of a pipeline.
    #[serde(rename="SingleCommand")]
    Single(T),
}

/// Type alias for the default `PipeableCommand` representation.
pub type DefaultPipeableCommand =
    ShellPipeableCommand<String, TopLevelWord<String>, TopLevelCommand<String>>;

/// Commands that can be used within a pipeline.
///
/// Generic over the representations of function names, simple commands,
/// compound commands, and function bodies.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum PipeableCommand<N, S, C, F> {
    /// The simplest possible command: an executable with arguments,
    /// environment variable assignments, and redirections.
    #[serde(rename="SimpleCommand")]
    Simple(S),
    /// A class of commands where redirection is applied to a command group.
    Compound(C),
    /// A function definition, associating a name with a group of commands,
    /// e.g. `function foo() { echo foo function; }`.
    FunctionDef(N, F),
}

/// A type alias for the default hiearchy for representing compound shell commands.
pub type ShellCompoundCommand<T, W, C> = CompoundCommand<CompoundCommandKind<T, W, C>, Redirect<W>>;

/// Type alias for the default `CompoundCommandKind` representation.
pub type DefaultCompoundCommand =
    ShellCompoundCommand<String, TopLevelWord<String>, TopLevelCommand<String>>;

/// A class of commands where redirection is applied to a command group.
///
/// Generic over the representation of a type of compound command, and the
/// representation of a redirect.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
pub struct CompoundCommand<T, R> {
    /// The specific kind of compound command.
    pub kind: T,
    /// Any redirections to be applied to the entire compound command
    pub io: Vec<R>,
}

/// Type alias for the default `CompoundCommandKind` representation.
pub type DefaultCompoundCommandKind =
    CompoundCommandKind<String, TopLevelWord<String>, TopLevelCommand<String>>;

/// A specific kind of a `CompoundCommand`.
///
/// Generic over the representation of shell words and commands.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum CompoundCommandKind<V, W, C> {
    /// A group of commands that should be executed in the current environment.
    Brace(Vec<C>),
    /// A group of commands that should be executed in a subshell environment.
    Subshell(Vec<C>),
    /// A command that executes its body as long as its guard exits successfully.
    While(GuardBodyPair<C>),
    /// A command that executes its body as until as its guard exits unsuccessfully.
    Until(GuardBodyPair<C>),
    /// A conditional command that runs the respective command branch when a
    /// certain of the first condition that exits successfully.
    If {
        /// A list of conditional branch-body pairs.
        conditionals: Vec<GuardBodyPair<C>>,
        /// An else part to run if no other conditional was taken.
        else_branch: Option<Vec<C>>,
    },
    /// A command that binds a variable to a number of provided words and runs
    /// its body once for each binding.
    For {
        /// The variable to bind to each of the specified words.
        var: V,
        /// The words to bind to the specified variable one by one.
        words: Option<Vec<W>>,
        /// The body to run with the variable binding.
        body: Vec<C>,
    },
    /// A command that behaves much like a `match` statment in Rust, running
    /// a branch of commands if a specified word matches another literal or
    /// glob pattern.
    Case {
        /// The word on which to check for pattern matches.
        word: W,
        /// The arms to match against.
        arms: Vec<PatternBodyPair<W, C>>,
    },
}

/// Represents a parsed redirect or a defined environment variable at the start
/// of a command.
///
/// Because the order in which redirects are defined may be significant for
/// execution, the parser will preserve the order in which they were parsed.
/// Thus we need a wrapper like this to disambiguate what was encountered in
/// the source program.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum RedirectOrEnvVar<R, V, W> {
    /// A parsed redirect before a command was encountered.
    Redirect(R),
    /// A parsed environment variable, e.g. `foo=[bar]`.
    EnvVar(V, Option<W>),
}

/// Represents a parsed redirect or a defined command or command argument.
///
/// Because the order in which redirects are defined may be significant for
/// execution, the parser will preserve the order in which they were parsed.
/// Thus we need a wrapper like this to disambiguate what was encountered in
/// the source program.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum RedirectOrCmdWord<R, W> {
    /// A parsed redirect after a command was encountered.
    Redirect(R),
    /// A parsed command name or argument.
    CmdWord(W),
}

/// Type alias for the default `SimpleCommand` representation.
pub type DefaultSimpleCommand =
    SimpleCommand<String, TopLevelWord<String>, Redirect<TopLevelWord<String>>>;

/// The simplest possible command: an executable with arguments,
/// environment variable assignments, and redirections.
///
/// Generic over representations of variable names, shell words, and redirects.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
pub struct SimpleCommand<V, W, R> {
    /// Redirections or environment variables that occur before any command
    /// in the order they were parsed.
    pub redirects_or_env_vars: Vec<RedirectOrEnvVar<R, V, W>>,
    /// Redirections or command name/argumetns in the order they were parsed.
    pub redirects_or_cmd_words: Vec<RedirectOrCmdWord<R, W>>,
}

/// Type alias for the default `Arithmetic` representation.
pub type DefaultArithmetic = Arithmetic<String>;

/// Represents an expression within an arithmetic subsitution.
///
/// Generic over the representation of a variable name.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag="t", content="v")]
pub enum Arithmetic<T> {
    /// The value of a variable, e.g. `$var` or `var`.
    #[serde(rename="VarArithmetic")]
    Var(T),
    /// A numeric literal such as `42` or `0xdeadbeef`.
    #[serde(rename="LiteralArithmetic")]
    Literal(isize),
    /// `left ** right`.
    Pow(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// Returns the current value of a variable,
    /// and then increments its value immediately after, e.g. `var++`
    PostIncr(T),
    /// Returns the current value of a variable,
    /// and then decrements its value immediately after, e.g. `var--`
    PostDecr(T),
    /// Increments the value of a variable and returns the new value, e.g. `++var`.
    PreIncr(T),
    /// Decrements the value of a variable and returns the new value, e.g. `--var`.
    PreDecr(T),
    /// Ensures the sign of the underlying result is positive, e.g. `+(1-2)`.
    UnaryPlus(Box<Arithmetic<T>>),
    /// Ensures the sign of the underlying result is negative, e.g. `-(1+2)`.
    UnaryMinus(Box<Arithmetic<T>>),
    /// Returns one if the underlying result is zero, or zero otherwise, e.g. `!expr`.
    LogicalNot(Box<Arithmetic<T>>),
    /// Flips all bits from the underlying result, e.g. `~expr`.
    BitwiseNot(Box<Arithmetic<T>>),
    /// `left * right`
    Mult(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left / right`
    Div(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left % right`
    Modulo(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left + right`
    Add(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left - right`
    Sub(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left << right`
    ShiftLeft(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left >> right`
    ShiftRight(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left < right`
    Less(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left <= right`
    LessEq(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left > right`
    Great(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left >= right`
    GreatEq(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left == right`
    Eq(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left != right`
    NotEq(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left & right`
    BitwiseAnd(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left ^ right`
    BitwiseXor(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left | right`
    BitwiseOr(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left && right`
    LogicalAnd(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `left || right`
    LogicalOr(Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// `first ? second : third`
    Ternary(Box<Arithmetic<T>>, Box<Arithmetic<T>>, Box<Arithmetic<T>>),
    /// Assigns the value of an underlying expression to a
    /// variable and returns the value, e.g. `x = 5`, or `x += 2`.
    #[serde(rename="AssignArithmetic")]
    Assign(T, Box<Arithmetic<T>>),
    /// `expr[, expr[, ...]]`
    Sequence(Vec<Arithmetic<T>>),
}

macro_rules! impl_top_level_cmd {
    ($(#[$attr:meta])* pub struct $Cmd:ident, $CmdList:ident, $Word:ident) => {
        $(#[$attr])*
        #[derive(Debug, PartialEq, Eq, Clone, ::serde::Serialize)]
        pub struct $Cmd<T>(pub Command<$CmdList<T, $Word<T>, $Cmd<T>>>);

        impl<T> ops::Deref for $Cmd<T> {
            type Target = Command<$CmdList<T, $Word<T>, $Cmd<T>>>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl<T> ops::DerefMut for $Cmd<T> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl<T> PartialEq<Command<$CmdList<T, $Word<T>, $Cmd<T>>>> for $Cmd<T> where T: PartialEq<T>
        {
            fn eq(&self, other: &Command<$CmdList<T, $Word<T>, $Cmd<T>>>) -> bool {
                &self.0 == other
            }
        }

        impl<T> From<Command<$CmdList<T, $Word<T>, $Cmd<T>>>> for $Cmd<T> {
            fn from(inner: Command<$CmdList<T, $Word<T>, $Cmd<T>>>) -> Self {
                $Cmd(inner)
            }
        }
    };
}

impl_top_level_cmd! {
    /// A top-level representation of a shell command. Uses `Rc` wrappers for function declarations.
    ///
    /// This wrapper unifies the provided top-level word representation,
    /// `ComplexWord`, and the top-level command representation, `Command`,
    /// while allowing them to be generic on their own.
    pub struct TopLevelCommand,
    CommandList,
    TopLevelWord
}

impl_top_level_cmd! {
    /// A top-level representation of a shell command. Uses `Arc` wrappers for function declarations.
    ///
    /// This wrapper unifies the provided top-level word representation,
    /// `ComplexWord`, and the top-level command representation, `Command`,
    /// while allowing them to be generic on their own.
    pub struct AtomicTopLevelCommand,
    AtomicCommandList,
    AtomicTopLevelWord
}

macro_rules! impl_top_level_word {
    ($(#[$attr:meta])* pub struct $Word:ident, $Cmd:ident) => {
        $(#[$attr])*
        #[derive(Debug, PartialEq, Eq, Clone, ::serde::Serialize)]
        pub struct $Word<T>(pub ShellWord<T, $Word<T>, $Cmd<T>>);

        impl<T> ops::Deref for $Word<T> {
            type Target = ShellWord<T, $Word<T>, $Cmd<T>>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl<T> ops::DerefMut for $Word<T> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl<T> PartialEq<ShellWord<T, $Word<T>, $Cmd<T>>> for $Word<T> where T: PartialEq<T> {
            fn eq(&self, other: &ShellWord<T, $Word<T>, $Cmd<T>>) -> bool {
                &self.0 == other
            }
        }

        impl<T> From<ShellWord<T, $Word<T>, $Cmd<T>>> for $Word<T> {
            fn from(inner: ShellWord<T, $Word<T>, $Cmd<T>>) -> Self {
                $Word(inner)
            }
        }
    };
}

impl_top_level_word! {
    /// A top-level representation of a shell word. Uses `Rc` wrappers for function declarations.
    ///
    /// This wrapper unifies the provided top-level word representation,
    /// `ComplexWord`, and the top-level command representation, `Command`,
    /// while allowing them to be generic on their own.
    pub struct TopLevelWord,
    TopLevelCommand
}

impl_top_level_word! {
    /// A top-level representation of a shell word. Uses `Arc` wrappers for function declarations.
    ///
    /// This wrapper unifies the provided top-level word representation,
    /// `ComplexWord`, and the top-level command representation, `Command`,
    /// while allowing them to be generic on their own.
    pub struct AtomicTopLevelWord,
    AtomicTopLevelCommand
}

impl<T: fmt::Display> fmt::Display for Parameter<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        use self::Parameter::*;

        match *self {
            At => fmt.write_str("$@"),
            Star => fmt.write_str("$*"),
            Pound => fmt.write_str("$#"),
            Question => fmt.write_str("$?"),
            Dash => fmt.write_str("$-"),
            Dollar => fmt.write_str("$$"),
            Bang => fmt.write_str("$!"),

            Var(ref p) => write!(fmt, "${{{}}}", p),
            Positional(p) => {
                if p <= 9 {
                    write!(fmt, "${}", p)
                } else {
                    write!(fmt, "${{{}}}", p)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_display_parameter() {
        use super::ComplexWord::Single;
        use super::Parameter::*;
        use super::SimpleWord::Param;
        use super::TopLevelWord;
        use super::Word::Simple;
        use crate::lexer::Lexer;
        use crate::parse::DefaultParser;

        let params = vec![
            At,
            Star,
            Pound,
            Question,
            Dash,
            Dollar,
            Bang,
            Positional(0),
            Positional(10),
            Positional(100),
            Var(String::from("foo_bar123")),
        ];

        for p in params {
            let src = p.to_string();
            let correct = TopLevelWord(Single(Simple(Param(p))));

            let parsed = match DefaultParser::new(Lexer::new(src.chars())).word() {
                Ok(Some(w)) => w,
                Ok(None) => panic!("The source \"{}\" generated from the command `{:#?}` failed to parse as anything", src, correct),
                Err(e) => panic!("The source \"{}\" generated from the command `{:#?}` failed to parse: {}", src, correct, e),
            };

            if correct != parsed {
                panic!(
                    "The source \"{}\" generated from the command `{:#?}` was parsed as `{:#?}`",
                    src, correct, parsed
                );
            }
        }
    }
}
