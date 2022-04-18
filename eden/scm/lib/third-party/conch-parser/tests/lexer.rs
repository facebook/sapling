#![deny(rust_2018_idioms)]
use conch_parser::lexer::Lexer;
use conch_parser::token::Token::*;
use conch_parser::token::{Positional, Token};

macro_rules! check_tok {
    ($fn_name:ident, $tok:expr) => {
        #[test]
        #[allow(non_snake_case)]
        fn $fn_name() {
            let s = format!("{}", $tok);
            let mut lex = Lexer::new(s.chars());
            assert_eq!($tok, lex.next().unwrap());
        }
    };
}

macro_rules! lex_str {
    ($fn_name:ident, $s:expr, $($tok:expr),+ ) => {
        #[test]
        #[allow(non_snake_case)]
        fn $fn_name() {
            let lex = Lexer::new($s.chars());
            let tokens: Vec<Token> = lex.collect();
            assert_eq!(tokens, vec!( $($tok),+ ));
        }
    }
}

check_tok!(check_Newline, Newline);
check_tok!(check_ParenOpen, ParenOpen);
check_tok!(check_ParenClose, ParenClose);
check_tok!(check_CurlyOpen, CurlyOpen);
check_tok!(check_CurlyClose, CurlyClose);
check_tok!(check_SquareOpen, SquareOpen);
check_tok!(check_SquareClose, SquareClose);
check_tok!(check_Dollar, Dollar);
check_tok!(check_Bang, Bang);
check_tok!(check_Semi, Semi);
check_tok!(check_Amp, Amp);
check_tok!(check_Less, Less);
check_tok!(check_Great, Great);
check_tok!(check_Pipe, Pipe);
check_tok!(check_Tilde, Tilde);
check_tok!(check_Star, Star);
check_tok!(check_Question, Question);
check_tok!(check_Percent, Percent);
check_tok!(check_Dash, Dash);
check_tok!(check_Equals, Equals);
check_tok!(check_Plus, Plus);
check_tok!(check_Colon, Colon);
check_tok!(check_At, At);
check_tok!(check_Caret, Caret);
check_tok!(check_Slash, Slash);
check_tok!(check_Comma, Comma);
check_tok!(check_Pound, Pound);
check_tok!(check_DoubleQuote, DoubleQuote);
check_tok!(check_Backtick, Backtick);
check_tok!(check_AndIf, AndIf);
check_tok!(check_OrIf, OrIf);
check_tok!(check_DSemi, DSemi);
check_tok!(check_DLess, DLess);
check_tok!(check_DGreat, DGreat);
check_tok!(check_GreatAnd, GreatAnd);
check_tok!(check_LessAnd, LessAnd);
check_tok!(check_DLessDash, DLessDash);
check_tok!(check_Clobber, Clobber);
check_tok!(check_LessGreat, LessGreat);
check_tok!(check_Whitespace, Whitespace(String::from(" \t\r")));
check_tok!(check_Name, Name(String::from("abc_23_defg")));
check_tok!(check_Literal, Literal(String::from("5abcdefg80hijklmnop")));
check_tok!(check_ParamPositional, ParamPositional(Positional::Nine));

lex_str!(check_greedy_Amp, "&&&", AndIf, Amp);
lex_str!(check_greedy_Pipe, "|||", OrIf, Pipe);
lex_str!(check_greedy_Semi, ";;;", DSemi, Semi);
lex_str!(check_greedy_Less, "<<<", DLess, Less);
lex_str!(check_greedy_Great, ">>>", DGreat, Great);
lex_str!(check_greedy_Less2, "<<<-", DLess, Less, Dash);

lex_str!(
    check_bad_Assigmnent_and_value,
    "5foobar=test",
    Literal(String::from("5foobar")),
    Equals,
    Name(String::from("test"))
);

lex_str!(
    check_Literal_and_Name_combo,
    "hello 5asdf5_ 6world __name ^.abc _test2",
    Name(String::from("hello")),
    Whitespace(String::from(" ")),
    Literal(String::from("5asdf5_")),
    Whitespace(String::from(" ")),
    Literal(String::from("6world")),
    Whitespace(String::from(" ")),
    Name(String::from("__name")),
    Whitespace(String::from(" ")),
    Caret,
    Literal(String::from(".abc")),
    Whitespace(String::from(" ")),
    Name(String::from("_test2"))
);

lex_str!(check_escape_Backslash, "\\\\", Backslash, Backslash);
lex_str!(check_escape_AndIf, "\\&&", Backslash, Amp, Amp);
lex_str!(check_escape_DSemi, "\\;;", Backslash, Semi, Semi);
lex_str!(check_escape_DLess, "\\<<", Backslash, Less, Less);
lex_str!(check_escape_DLessDash, "\\<<-", Backslash, Less, Less, Dash);
lex_str!(
    check_escape_ParamPositional,
    "\\$0",
    Backslash,
    Dollar,
    Literal(String::from("0"))
);
lex_str!(
    check_escape_Whitespace,
    "\\  ",
    Backslash,
    Whitespace(String::from(" ")),
    Whitespace(String::from(" "))
);
lex_str!(
    check_escape_Name,
    "\\ab",
    Backslash,
    Name(String::from("a")),
    Name(String::from("b"))
);
lex_str!(
    check_escape_Literal,
    "\\13",
    Backslash,
    Literal(String::from("1")),
    Literal(String::from("3"))
);

lex_str!(
    check_no_tokens_lost,
    "word\\'",
    Name(String::from("word")),
    Backslash,
    SingleQuote
);
