//! This module defines the tokens of the shell language.

use self::Token::*;
use std::fmt;

/// The inner representation of a positional parameter.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum Positional {
    /// $0
    Zero,
    /// $1
    One,
    /// $2
    Two,
    /// $3
    Three,
    /// $4
    Four,
    /// $5
    Five,
    /// $6
    Six,
    /// $7
    Seven,
    /// $8
    Eight,
    /// $9
    Nine,
}

impl Positional {
    /// Converts a `Positional` as a numeric representation
    pub fn as_num(&self) -> u8 {
        match *self {
            Positional::Zero => 0,
            Positional::One => 1,
            Positional::Two => 2,
            Positional::Three => 3,
            Positional::Four => 4,
            Positional::Five => 5,
            Positional::Six => 6,
            Positional::Seven => 7,
            Positional::Eight => 8,
            Positional::Nine => 9,
        }
    }

    /// Attempts to convert a number to a `Positional` representation
    pub fn from_num(num: u8) -> Option<Self> {
        match num {
            0 => Some(Positional::Zero),
            1 => Some(Positional::One),
            2 => Some(Positional::Two),
            3 => Some(Positional::Three),
            4 => Some(Positional::Four),
            5 => Some(Positional::Five),
            6 => Some(Positional::Six),
            7 => Some(Positional::Seven),
            8 => Some(Positional::Eight),
            9 => Some(Positional::Nine),
            _ => None,
        }
    }
}

impl Into<u8> for Positional {
    fn into(self) -> u8 {
        self.as_num()
    }
}

/// The representation of (context free) shell tokens.
#[derive(PartialEq, Eq, Debug, Clone)]
pub enum Token {
    /// \n
    Newline,

    /// (
    ParenOpen,
    /// )
    ParenClose,
    /// {
    CurlyOpen,
    /// }
    CurlyClose,
    /// [
    SquareOpen,
    /// ]
    SquareClose,

    /// !
    Bang,
    /// ~
    Tilde,
    /// \#
    Pound,
    /// *
    Star,
    /// ?
    Question,
    /// \\
    Backslash,
    /// %
    Percent,
    /// \-
    Dash,
    /// \=
    Equals,
    /// +
    Plus,
    /// :
    Colon,
    /// @
    At,
    /// ^
    Caret,
    /// /
    Slash,
    /// ,
    Comma,

    /// '
    SingleQuote,
    /// "
    DoubleQuote,
    /// `
    Backtick,

    /// ;
    Semi,
    /// &
    Amp,
    /// |
    Pipe,
    /// &&
    AndIf,
    /// ||
    OrIf,
    /// ;;
    DSemi,

    /// <
    Less,
    /// \>
    Great,
    /// <<
    DLess,
    /// \>>
    DGreat,
    /// \>&
    GreatAnd,
    /// <&
    LessAnd,
    /// <<-
    DLessDash,
    /// \>|
    Clobber,
    /// <>
    LessGreat,

    /// $
    Dollar,
    /// $0, $1, ..., $9
    ///
    /// Must be its own token to avoid lumping the positional parameter
    /// as a `Literal` if the parameter is concatenated to something.
    ParamPositional(Positional),

    /// Any string of whitespace characters NOT including a newline.
    Whitespace(String),

    /// Any literal delimited by whitespace.
    Literal(String),
    /// A `Literal` capable of being used as a variable or function name. According to the POSIX
    /// standard it should only contain alphanumerics or underscores, and does not start with a digit.
    Name(String),
}

impl fmt::Display for Token {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}", self.as_str())
    }
}

impl Token {
    /// Returns if the token's length is zero.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the number of characters it took to recognize a token.
    pub fn len(&self) -> usize {
        self.as_str().len()
    }

    /// Indicates whether a word can be delimited by this token
    /// when the token is **not** quoted or escaped.
    pub fn is_word_delimiter(&self) -> bool {
        match *self {
            Newline | ParenOpen | ParenClose | Semi | Amp | Less | Great | Pipe | AndIf | OrIf
            | DSemi | DLess | DGreat | GreatAnd | LessAnd | DLessDash | Clobber | LessGreat
            | Whitespace(_) => true,

            Bang | Star | Question | Backslash | SingleQuote | DoubleQuote | Backtick | Percent
            | Dash | Equals | Plus | Colon | At | Caret | Slash | Comma | CurlyOpen
            | CurlyClose | SquareOpen | SquareClose | Dollar | Tilde | Pound | Name(_)
            | Literal(_) | ParamPositional(_) => false,
        }
    }

    /// Gets a representation of the token as a string slice.
    pub fn as_str(&self) -> &str {
        match *self {
            Newline => "\n",
            ParenOpen => "(",
            ParenClose => ")",
            CurlyOpen => "{",
            CurlyClose => "}",
            SquareOpen => "[",
            SquareClose => "]",
            Dollar => "$",
            Bang => "!",
            Semi => ";",
            Amp => "&",
            Less => "<",
            Great => ">",
            Pipe => "|",
            Tilde => "~",
            Pound => "#",
            Star => "*",
            Question => "?",
            Backslash => "\\",
            Percent => "%",
            Dash => "-",
            Equals => "=",
            Plus => "+",
            Colon => ":",
            At => "@",
            Caret => "^",
            Slash => "/",
            Comma => ",",
            SingleQuote => "\'",
            DoubleQuote => "\"",
            Backtick => "`",
            AndIf => "&&",
            OrIf => "||",
            DSemi => ";;",
            DLess => "<<",
            DGreat => ">>",
            GreatAnd => ">&",
            LessAnd => "<&",
            DLessDash => "<<-",
            Clobber => ">|",
            LessGreat => "<>",

            ParamPositional(Positional::Zero) => "$0",
            ParamPositional(Positional::One) => "$1",
            ParamPositional(Positional::Two) => "$2",
            ParamPositional(Positional::Three) => "$3",
            ParamPositional(Positional::Four) => "$4",
            ParamPositional(Positional::Five) => "$5",
            ParamPositional(Positional::Six) => "$6",
            ParamPositional(Positional::Seven) => "$7",
            ParamPositional(Positional::Eight) => "$8",
            ParamPositional(Positional::Nine) => "$9",

            Whitespace(ref s) | Name(ref s) | Literal(ref s) => s,
        }
    }
}
