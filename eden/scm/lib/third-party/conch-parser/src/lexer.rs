//! This module defines a lexer to recognize tokens of the shell language.

use self::TokenOrLiteral::*;
use super::token::Token::*;
use super::token::{Positional, Token};
use std::iter::{Fuse, Peekable};

#[derive(PartialEq, Eq, Debug, Clone)]
enum TokenOrLiteral {
    Tok(Token),
    Escaped(Option<Token>),
    Lit(char),
}

/// Converts raw characters into shell tokens.
#[must_use = "`Lexer` is lazy and does nothing unless consumed"]
#[derive(Clone, Debug)]
pub struct Lexer<I: Iterator<Item = char>> {
    inner: Peekable<Fuse<I>>,
    peeked: Option<TokenOrLiteral>,
}

impl<I: Iterator<Item = char>> Lexer<I> {
    /// Creates a new Lexer from any char iterator.
    pub fn new(iter: I) -> Lexer<I> {
        Lexer {
            inner: iter.fuse().peekable(),
            peeked: None,
        }
    }

    #[inline]
    fn next_is(&mut self, c: char) -> bool {
        let is = self.inner.peek() == Some(&c);
        if is {
            self.inner.next();
        }
        is
    }

    fn next_internal(&mut self) -> Option<TokenOrLiteral> {
        if self.peeked.is_some() {
            return self.peeked.take();
        }

        let cur = match self.inner.next() {
            Some(c) => c,
            None => return None,
        };

        let tok = match cur {
            '\n' => Newline,
            '!' => Bang,
            '~' => Tilde,
            '#' => Pound,
            '*' => Star,
            '?' => Question,
            '%' => Percent,
            '-' => Dash,
            '=' => Equals,
            '+' => Plus,
            ':' => Colon,
            '@' => At,
            '^' => Caret,
            '/' => Slash,
            ',' => Comma,

            // Make sure that we treat the next token as a single character,
            // preventing multi-char tokens from being recognized. This is
            // important because something like `\&&` would mean that the
            // first & is a literal while the second retains its properties.
            // We will let the parser deal with what actually becomes a literal.
            '\\' => {
                return Some(Escaped(
                    self.inner
                        .next()
                        .and_then(|c| Lexer::new(std::iter::once(c)).next()),
                ))
            }

            '\'' => SingleQuote,
            '"' => DoubleQuote,
            '`' => Backtick,

            ';' => {
                if self.next_is(';') {
                    DSemi
                } else {
                    Semi
                }
            }
            '&' => {
                if self.next_is('&') {
                    AndIf
                } else {
                    Amp
                }
            }
            '|' => {
                if self.next_is('|') {
                    OrIf
                } else {
                    Pipe
                }
            }

            '(' => ParenOpen,
            ')' => ParenClose,
            '{' => CurlyOpen,
            '}' => CurlyClose,
            '[' => SquareOpen,
            ']' => SquareClose,

            '$' => {
                // Positional parameters are 0-9, so we only
                // need to check a single digit ahead.
                let positional = match self.inner.peek() {
                    Some(&'0') => Some(Positional::Zero),
                    Some(&'1') => Some(Positional::One),
                    Some(&'2') => Some(Positional::Two),
                    Some(&'3') => Some(Positional::Three),
                    Some(&'4') => Some(Positional::Four),
                    Some(&'5') => Some(Positional::Five),
                    Some(&'6') => Some(Positional::Six),
                    Some(&'7') => Some(Positional::Seven),
                    Some(&'8') => Some(Positional::Eight),
                    Some(&'9') => Some(Positional::Nine),
                    _ => None,
                };

                match positional {
                    Some(p) => {
                        self.inner.next(); // Consume the character we just peeked
                        ParamPositional(p)
                    }
                    None => Dollar,
                }
            }

            '<' => {
                if self.next_is('<') {
                    if self.next_is('-') {
                        DLessDash
                    } else {
                        DLess
                    }
                } else if self.next_is('&') {
                    LessAnd
                } else if self.next_is('>') {
                    LessGreat
                } else {
                    Less
                }
            }

            '>' => {
                if self.next_is('&') {
                    GreatAnd
                } else if self.next_is('>') {
                    DGreat
                } else if self.next_is('|') {
                    Clobber
                } else {
                    Great
                }
            }

            // Newlines are valid whitespace, however, we want to tokenize them separately!
            c if c.is_whitespace() => {
                let mut buf = String::new();
                buf.push(c);

                // NB: Can't use filter here because it will advance the iterator too far.
                while let Some(&c) = self.inner.peek() {
                    if c.is_whitespace() && c != '\n' {
                        self.inner.next();
                        buf.push(c);
                    } else {
                        break;
                    }
                }

                Whitespace(buf)
            }

            c => return Some(Lit(c)),
        };

        Some(Tok(tok))
    }
}

impl<I: Iterator<Item = char>> Iterator for Lexer<I> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        fn name_start_char(c: char) -> bool {
            c == '_' || c.is_alphabetic()
        }

        fn is_digit(c: char) -> bool {
            c.is_digit(10)
        }

        fn name_char(c: char) -> bool {
            is_digit(c) || name_start_char(c)
        }

        match self.next_internal() {
            None => None,
            Some(Tok(t)) => Some(t),
            Some(Escaped(t)) => {
                debug_assert_eq!(self.peeked, None);
                self.peeked = t.map(Tok);
                Some(Backslash)
            }

            Some(Lit(c)) => {
                let is_name = name_start_char(c);
                let mut word = String::new();
                word.push(c);

                loop {
                    match self.next_internal() {
                        // If we hit a token, delimit the current word w/o losing the token
                        Some(tok @ Tok(_)) | Some(tok @ Escaped(_)) => {
                            debug_assert_eq!(self.peeked, None);
                            self.peeked = Some(tok);
                            break;
                        }

                        // Make sure we delimit valid names whenever a non-name char comes along
                        Some(Lit(c)) if is_name && !name_char(c) => {
                            debug_assert_eq!(self.peeked, None);
                            self.peeked = Some(Lit(c));
                            return Some(Name(word));
                        }

                        // Otherwise, keep consuming characters for the literal
                        Some(Lit(c)) => word.push(c),

                        None => break,
                    }
                }

                if is_name {
                    Some(Name(word))
                } else {
                    Some(Literal(word))
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // The number of actual tokens we yield will never exceed
        // the amount of characters we are processing. In practice
        // the caller will probably see a lot fewer tokens than
        // number of characters processed, however, they can prepare
        // themselves for the worst possible case. A high estimate
        // is better than no estimate.
        let (_, hi) = self.inner.size_hint();
        let low = if self.peeked.is_some() { 1 } else { 0 };
        (low, hi)
    }
}
