//! An module for easily iterating over a `Token` stream.

use crate::parse::SourcePos;
use crate::token::Token;
use crate::token::Token::*;
use std::iter as std_iter;
use std::mem;

/// Indicates an error such that EOF was encountered while some unmatched
/// tokens were still pending. The error stores the unmatched token
/// and the position where it appears in the source.
#[derive(Debug)]
pub struct UnmatchedError(pub Token, pub SourcePos);

/// An internal variant that indicates if a token should be yielded
/// or the current position updated to some value.
#[derive(Debug)]
enum TokenOrPos {
    /// A consumed token which should be yielded.
    Tok(Token),
    /// The current position should be updated to the contained value.
    Pos(SourcePos),
}

impl TokenOrPos {
    /// Returns `true` if `self` is a `Tok` value.
    #[inline]
    fn is_tok(&self) -> bool {
        match *self {
            TokenOrPos::Tok(_) => true,
            TokenOrPos::Pos(_) => false,
        }
    }
}

/// An iterator that can track its internal position in the stream.
pub trait PositionIterator: Iterator {
    /// Get the current position of the iterator.
    fn pos(&self) -> SourcePos;
}

impl<'a, T: PositionIterator> PositionIterator for &'a mut T {
    fn pos(&self) -> SourcePos {
        (**self).pos()
    }
}

/// An iterator that supports peeking a single element in the stream.
///
/// Identical to `std::iter::Peekable` but in a trait form.
pub trait PeekableIterator: Iterator {
    /// Peek at the next item, identical to `std::iter::Peekable::peek`.
    fn peek(&mut self) -> Option<&Self::Item>;
}

impl<'a, T: PeekableIterator> PeekableIterator for &'a mut T {
    fn peek(&mut self) -> Option<&Self::Item> {
        (**self).peek()
    }
}

impl<I: Iterator> PeekableIterator for std_iter::Peekable<I> {
    fn peek(&mut self) -> Option<&Self::Item> {
        std_iter::Peekable::peek(self)
    }
}

/// A marker trait that unifies `PeekableIterator` and `PositionIterator`.
pub trait PeekablePositionIterator: PeekableIterator + PositionIterator {}
impl<T: PeekableIterator + PositionIterator> PeekablePositionIterator for T {}

/// A convenience trait for converting `Token` iterators into other sub-iterators.
pub trait TokenIterator: Sized + PeekablePositionIterator<Item = Token> {
    /// Returns an iterator that yields at least one token, but continues to yield
    /// tokens until all matching cases of single/double quotes, backticks,
    /// ${ }, $( ), or ( ) are found.
    fn balanced(&mut self) -> Balanced<&mut Self> {
        Balanced::new(self, None)
    }

    /// Returns an iterator that yields tokens up to when a (closing) single quote
    /// is reached (assuming that the caller has reached the opening quote and
    /// wishes to continue up to but not including the closing quote).
    fn single_quoted(&mut self, pos: SourcePos) -> Balanced<&mut Self> {
        Balanced::new(self, Some((SingleQuote, pos)))
    }

    /// Returns an iterator that yields tokens up to when a (closing) double quote
    /// is reached (assuming that the caller has reached the opening quote and
    /// wishes to continue up to but not including the closing quote).
    fn double_quoted(&mut self, pos: SourcePos) -> Balanced<&mut Self> {
        Balanced::new(self, Some((DoubleQuote, pos)))
    }

    /// Returns an iterator that yields tokens up to when a (closing) backtick
    /// is reached (assuming that the caller has reached the opening backtick and
    /// wishes to continue up to but not including the closing backtick).
    fn backticked(&mut self, pos: SourcePos) -> Balanced<&mut Self> {
        Balanced::new(self, Some((Backtick, pos)))
    }

    /// Returns an iterator that yields tokens up to when a (closing) backtick
    /// is reached (assuming that the caller has reached the opening backtick and
    /// wishes to continue up to but not including the closing backtick).
    /// Any backslashes followed by \, $, or ` are removed from the stream.
    fn backticked_remove_backslashes(
        &mut self,
        pos: SourcePos,
    ) -> BacktickBackslashRemover<&mut Self> {
        BacktickBackslashRemover::new(self.backticked(pos))
    }
}

/// Convenience trait for `Token` iterators which could be "rewound" so that
/// they can yield tokens that were already pulled out of their stream.
trait RewindableTokenIterator {
    /// Rewind the iterator with the provided tokens. Vector should contain
    /// the tokens in the order they should be yielded.
    fn rewind(&mut self, tokens: Vec<TokenOrPos>);

    /// Grab the next token (or internal position) that should be buffered
    /// by the caller.
    fn next_token_or_pos(&mut self) -> Option<TokenOrPos>;
}

/// A Token iterator that keeps track of how many lines have been read.
#[must_use = "iterator adaptors are lazy and do nothing unless consumed"]
#[derive(Debug)]
pub struct TokenIter<I> {
    /// The underlying token iterator being wrapped. Iterator is fused to avoid
    /// inconsistent behavior when doing multiple peek ahead operations.
    iter: std_iter::Fuse<I>,
    /// Any tokens that were previously yielded but to be consumed later, stored
    /// as a stack. Intersperced between the tokens are any changes to the current
    /// position that should be applied. This is useful for situations where the
    /// parser may have removed certain tokens (e.g. \ when unescaping), but we still
    /// want to keep track of token positions in the actual source.
    prev_buffered: Vec<TokenOrPos>,
    /// The current position in the source that we have consumed up to
    pos: SourcePos,
}

impl<I: Iterator<Item = Token>> PositionIterator for TokenIter<I> {
    fn pos(&self) -> SourcePos {
        self.pos
    }
}

impl<I: Iterator<Item = Token>> PeekableIterator for TokenIter<I> {
    fn peek(&mut self) -> Option<&Self::Item> {
        // Peek the next token, then drop the wrapper to get the token pushed
        // back on our buffer. Not the clearest solution, but gets around
        // the borrow checker.
        let _ = self.multipeek().peek_next()?;

        if let Some(&TokenOrPos::Tok(ref t)) = self.prev_buffered.last() {
            Some(t)
        } else {
            unreachable!("unexpected state: peeking next token failed. This is a bug!")
        }
    }
}

impl<I: Iterator<Item = Token>> Iterator for TokenIter<I> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        let mut ret = None;
        loop {
            // Make sure we update our current position before continuing.
            match self.next_token_or_pos() {
                Some(TokenOrPos::Tok(next)) => {
                    self.pos.advance(&next);
                    ret = Some(next);
                    break;
                }

                Some(TokenOrPos::Pos(_)) => panic!("unexpected state. This is a bug!"),
                None => break,
            }
        }

        // Make sure we update our position according to any trailing `Pos` points.
        // The parser expects that polling our current position will give it the
        // position of the next token we will yield. If we perform this check right
        // before yielding the next token, the parser will believe that token appears
        // much earlier in the source than it actually does.
        self.updated_buffered_pos();
        ret
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (low_hint, hi) = self.iter.size_hint();
        let low = if self.prev_buffered.is_empty() {
            low_hint
        } else {
            self.prev_buffered.len()
        };
        (low, hi)
    }
}

impl<I: Iterator<Item = Token>> RewindableTokenIterator for TokenIter<I> {
    fn rewind(&mut self, tokens: Vec<TokenOrPos>) {
        self.buffer_tokens_and_positions_to_yield_first(tokens, None);
    }

    fn next_token_or_pos(&mut self) -> Option<TokenOrPos> {
        self.prev_buffered
            .pop()
            .or_else(|| self.iter.next().map(TokenOrPos::Tok))
    }
}

impl<I: Iterator<Item = Token>> TokenIterator for TokenIter<I> {}

impl<I: Iterator<Item = Token>> TokenIter<I> {
    /// Creates a new TokenIter from another Token iterator.
    pub fn new(iter: I) -> TokenIter<I> {
        TokenIter {
            iter: iter.fuse(),
            prev_buffered: Vec::new(),
            pos: SourcePos::new(),
        }
    }

    /// Creates a new TokenIter from another Token iterator and an initial position.
    pub fn with_position(iter: I, pos: SourcePos) -> TokenIter<I> {
        let mut iter = TokenIter::new(iter);
        iter.pos = pos;
        iter
    }

    /// Return a wrapper which allows for arbitrary look ahead. Dropping the
    /// wrapper will restore the internal stream back to what it was.
    pub fn multipeek(&mut self) -> Multipeek<'_> {
        Multipeek::new(self)
    }

    /// Update the current position based on any buffered state.
    ///
    /// This allows us to always correctly report the position of the next token
    /// we are about to yield.
    fn updated_buffered_pos(&mut self) {
        while let Some(&TokenOrPos::Pos(pos)) = self.prev_buffered.last() {
            self.prev_buffered.pop();
            self.pos = pos;
        }
    }

    /// Accepts a vector of tokens (and positions) to be yielded completely before the
    /// inner iterator is advanced further. The optional `buf_start` (if provided)
    /// indicates what the iterator's position should have been if we were to naturally
    /// yield the provided buffer.
    fn buffer_tokens_and_positions_to_yield_first(
        &mut self,
        mut tokens: Vec<TokenOrPos>,
        token_start: Option<SourcePos>,
    ) {
        self.prev_buffered.reserve(tokens.len() + 1);

        // Push the current position further up the stack since we want to
        // restore it before yielding any previously-peeked tokens.
        if token_start.is_some() {
            self.prev_buffered.push(TokenOrPos::Pos(self.pos));
        }

        // Buffer the newly provided tokens in reverse since we are using a stack
        tokens.reverse();
        self.prev_buffered.extend(tokens);

        // Set our position to what it should be as we yield the buffered tokens
        if let Some(p) = token_start {
            self.pos = p;
        }

        self.updated_buffered_pos();
    }

    /// Accepts a vector of tokens to be yielded completely before the inner
    /// iterator is advanced further. The provided `buf_start` indicates
    /// what the iterator's position should have been if we were to naturally
    /// yield the provided buffer.
    pub fn buffer_tokens_to_yield_first(&mut self, buf: Vec<Token>, buf_start: SourcePos) {
        let tokens = buf.into_iter().map(TokenOrPos::Tok).collect();
        self.buffer_tokens_and_positions_to_yield_first(tokens, Some(buf_start));
    }

    /// Collects all tokens yielded by `TokenIter::backticked_remove_backslashes`
    /// and creates a `TokenIter` which will yield the collected tokens, and maintain
    /// the correct position of where each token appears in the original source,
    /// regardless of how many backslashes may have been removed since then.
    pub fn token_iter_from_backticked_with_removed_backslashes(
        &mut self,
        pos: SourcePos,
    ) -> Result<TokenIter<std_iter::Empty<Token>>, UnmatchedError> {
        BacktickBackslashRemover::create_token_iter(self.backticked(pos))
    }
}

/// A wrapper for peeking arbitrary amounts into a `Token` stream.
/// Inspired by the `Multipeek` implementation in the `itertools` crate.
#[must_use = "iterator adaptors are lazy and do nothing unless consumed"]
pub struct Multipeek<'a> {
    /// The underlying token iterator. This is pretty much just a `TokenIter`,
    /// but we use a trait object to avoid having a generic signature and
    /// make this wrapper more flexible.
    iter: &'a mut dyn RewindableTokenIterator,
    /// A buffer of values taken from the underlying iterator, in the order
    /// they were pulled.
    buf: Vec<TokenOrPos>,
}

impl<'a> Drop for Multipeek<'a> {
    fn drop(&mut self) {
        let tokens = mem::replace(&mut self.buf, Vec::new());
        self.iter.rewind(tokens);
    }
}

impl<'a> Multipeek<'a> {
    /// Wrap an iterator for arbitrary look-ahead.
    fn new(iter: &'a mut dyn RewindableTokenIterator) -> Self {
        Multipeek {
            iter,
            buf: Vec::new(),
        }
    }

    /// Public method for lazily peeking the next (unpeeked) value.
    /// Implemented as its own API instead of as an `Iterator` to avoid
    /// confusion with advancing the regular iterator.
    pub fn peek_next(&mut self) -> Option<&Token> {
        loop {
            match self.iter.next_token_or_pos() {
                Some(t) => {
                    let is_tok = t.is_tok();
                    self.buf.push(t);

                    if is_tok {
                        break;
                    }
                }
                None => return None,
            }
        }

        if let Some(&TokenOrPos::Tok(ref t)) = self.buf.last() {
            Some(t)
        } else {
            None
        }
    }
}

/// A wrapper which allows treating `TokenIter<I>` and `TokenIter<Empty<_>>` as
/// the same thing, even though they are technically different types.
#[must_use = "iterator adaptors are lazy and do nothing unless consumed"]
#[derive(Debug)]
pub enum TokenIterWrapper<I> {
    /// A `TokenIter` which holds an aribtrary `Iterator` over `Token`s.
    Regular(TokenIter<I>),
    /// A `TokenIter` which has all `Token`s buffered in memory, and thus
    /// has no underlying iterator.
    Buffered(TokenIter<std_iter::Empty<Token>>),
}

impl<I: Iterator<Item = Token>> PositionIterator for TokenIterWrapper<I> {
    fn pos(&self) -> SourcePos {
        match *self {
            TokenIterWrapper::Regular(ref inner) => inner.pos(),
            TokenIterWrapper::Buffered(ref inner) => inner.pos(),
        }
    }
}

impl<I: Iterator<Item = Token>> PeekableIterator for TokenIterWrapper<I> {
    fn peek(&mut self) -> Option<&Self::Item> {
        match *self {
            TokenIterWrapper::Regular(ref mut inner) => inner.peek(),
            TokenIterWrapper::Buffered(ref mut inner) => inner.peek(),
        }
    }
}

impl<I: Iterator<Item = Token>> Iterator for TokenIterWrapper<I> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        match *self {
            TokenIterWrapper::Regular(ref mut inner) => inner.next(),
            TokenIterWrapper::Buffered(ref mut inner) => inner.next(),
        }
    }
}

impl<I: Iterator<Item = Token>> TokenIterator for TokenIterWrapper<I> {}

impl<I: Iterator<Item = Token>> TokenIterWrapper<I> {
    /// Return a wrapper which allows for arbitrary look ahead. Dropping the
    /// wrapper will restore the internal stream back to what it was.
    pub fn multipeek(&mut self) -> Multipeek<'_> {
        match *self {
            TokenIterWrapper::Regular(ref mut inner) => inner.multipeek(),
            TokenIterWrapper::Buffered(ref mut inner) => inner.multipeek(),
        }
    }

    /// Delegates to `TokenIter::buffer_tokens_to_yield_first`.
    pub fn buffer_tokens_to_yield_first(&mut self, buf: Vec<Token>, buf_start: SourcePos) {
        match *self {
            TokenIterWrapper::Regular(ref mut inner) => {
                inner.buffer_tokens_to_yield_first(buf, buf_start)
            }
            TokenIterWrapper::Buffered(ref mut inner) => {
                inner.buffer_tokens_to_yield_first(buf, buf_start)
            }
        }
    }

    /// Delegates to `TokenIter::token_iter_from_backticked_with_removed_backslashes`.
    pub fn token_iter_from_backticked_with_removed_backslashes(
        &mut self,
        pos: SourcePos,
    ) -> Result<TokenIter<std_iter::Empty<Token>>, UnmatchedError> {
        match *self {
            TokenIterWrapper::Regular(ref mut inner) => {
                inner.token_iter_from_backticked_with_removed_backslashes(pos)
            }
            TokenIterWrapper::Buffered(ref mut inner) => {
                inner.token_iter_from_backticked_with_removed_backslashes(pos)
            }
        }
    }
}

/// An iterator that yields at least one token, but continues to yield
/// tokens until all matching cases of single/double quotes, backticks,
/// ${ }, $( ), or ( ) are found.
#[must_use = "iterator adaptors are lazy and do nothing unless consumed"]
#[derive(Debug)]
pub struct Balanced<I> {
    /// The underlying token iterator.
    iter: I,
    /// Any token we had to peek after a backslash but haven't yielded yet,
    /// as well as the position after it.
    escaped: Option<(Token, SourcePos)>,
    /// A stack of pending unmatched tokens we still must encounter.
    stack: Vec<(Token, SourcePos)>,
    /// Indicates if we should yield the final, outermost delimeter.
    skip_last_delimeter: bool,
    /// Makes the iterator *fused* by yielding None forever after we are done.
    done: bool,
    /// The current position of the iterator.
    pos: SourcePos,
}

impl<I: PositionIterator> Balanced<I> {
    /// Constructs a new balanced iterator.
    ///
    /// If no delimeter is given, a single token will be yielded, unless the
    /// first found token is an opening one (e.g. "), making the iterator yield
    /// tokens until its matching delimeter is found (the matching delimeter *will*
    /// be consumed).
    ///
    /// If a delimeter (and its position) is specified, tokens are yielded *up to*
    /// the delimeter, but the delimeter will be silently consumed.
    pub fn new(iter: I, delim: Option<(Token, SourcePos)>) -> Self {
        Balanced {
            escaped: None,
            skip_last_delimeter: delim.is_some(),
            stack: delim.map_or(Vec::new(), |d| vec![d]),
            done: false,
            pos: iter.pos(),
            iter,
        }
    }
}

impl<I: PeekablePositionIterator<Item = Token>> PositionIterator for Balanced<I> {
    fn pos(&self) -> SourcePos {
        self.pos
    }
}

impl<I: PeekablePositionIterator<Item = Token>> Iterator for Balanced<I> {
    type Item = Result<Token, UnmatchedError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((tok, pos)) = self.escaped.take() {
            self.pos = pos;
            return Some(Ok(tok));
        } else if self.done {
            return None;
        }

        if self.stack.last().map(|t| &t.0) == self.iter.peek() {
            let ret = self.iter.next().map(Ok);
            self.stack.pop();
            let stack_empty = self.stack.is_empty();
            self.done |= stack_empty;
            self.pos = self.iter.pos();
            if self.skip_last_delimeter && stack_empty {
                return None;
            } else {
                return ret;
            };
        }

        // Tokens between single quotes have no special meaning
        // so we should make sure we don't treat anything specially.
        if let Some(&(SingleQuote, pos)) = self.stack.last() {
            let ret = match self.iter.next() {
                // Closing SingleQuote should have been captured above
                Some(t) => Some(Ok(t)),
                // Make sure we indicate errors on missing closing quotes
                None => Some(Err(UnmatchedError(SingleQuote, pos))),
            };

            self.pos = self.iter.pos();
            return ret;
        }

        let cur_pos = self.iter.pos();
        let ret = match self.iter.next() {
            Some(Backslash) => {
                // Make sure that we indicate our position as before the escaped token,
                // and NOT as the underlying iterator's position, which will indicate the
                // position AFTER the escaped token (which we are buffering ourselves)
                self.pos = self.iter.pos();

                debug_assert_eq!(self.escaped, None);
                self.escaped = self.iter.next().map(|t| (t, self.iter.pos()));
                // Make sure we stop yielding tokens after the stored escaped token
                // otherwise we risk consuming one token too many!
                self.done |= self.stack.is_empty();
                return Some(Ok(Backslash));
            }

            Some(Backtick) => {
                self.stack.push((Backtick, cur_pos));
                Some(Ok(Backtick))
            }

            Some(SingleQuote) => {
                if self.stack.last().map(|t| &t.0) != Some(&DoubleQuote) {
                    self.stack.push((SingleQuote, cur_pos));
                }
                Some(Ok(SingleQuote))
            }

            Some(DoubleQuote) => {
                self.stack.push((DoubleQuote, cur_pos));
                Some(Ok(DoubleQuote))
            }

            Some(ParenOpen) => {
                self.stack.push((ParenClose, cur_pos));
                Some(Ok(ParenOpen))
            }

            Some(Dollar) => {
                let cur_pos = self.iter.pos(); // Want the pos of curly or paren, not $ here
                match self.iter.peek() {
                    Some(&CurlyOpen) => self.stack.push((CurlyClose, cur_pos)),
                    Some(&ParenOpen) => {} // Already handled by paren case above

                    // We have nothing further to match
                    _ => {
                        self.done |= self.stack.is_empty();
                    }
                }
                Some(Ok(Dollar))
            }

            Some(t) => {
                // If we aren't looking for any more delimeters we should only
                // consume a single token (since its balanced by nature)
                self.done |= self.stack.is_empty();
                Some(Ok(t))
            }

            None => match self.stack.pop() {
                // Its okay to hit EOF if everything is balanced so far
                None => {
                    self.done = true;
                    None
                }
                // But its not okay otherwise
                Some((ParenClose, pos)) => Some(Err(UnmatchedError(ParenOpen, pos))),
                Some((CurlyClose, pos)) => Some(Err(UnmatchedError(CurlyOpen, pos))),
                Some((delim, pos)) => Some(Err(UnmatchedError(delim, pos))),
            },
        };

        self.pos = self.iter.pos();
        ret
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Our best guess is as good as the internal token iterator's...
        self.iter.size_hint()
    }
}

/// A `Balanced` backtick `Token` iterator which removes all backslashes
/// from the stream that are followed by \, $, or `.
#[must_use = "iterator adaptors are lazy and do nothing unless consumed"]
#[derive(Debug)]
pub struct BacktickBackslashRemover<I> {
    /// The underlying token iterator.
    iter: Balanced<I>,
    peeked: Option<Result<Token, UnmatchedError>>,
    /// Makes the iterator *fused* by yielding None forever after we are done.
    done: bool,
}

impl<I> BacktickBackslashRemover<I> {
    /// Constructs a new balanced backtick iterator which removes all backslashes
    /// from the stream that are followed by \, $, or `.
    pub fn new(iter: Balanced<I>) -> Self {
        BacktickBackslashRemover {
            iter,
            peeked: None,
            done: false,
        }
    }
}

impl<I: PeekablePositionIterator<Item = Token>> BacktickBackslashRemover<I> {
    /// Collects all tokens yielded by `TokenIter::backticked_remove_backslashes`
    /// and creates a `TokenIter` which will yield the collected tokens, and maintain
    /// the correct position of where each token appears in the original source,
    /// regardless of how many backslashes may have been removed since then.
    fn create_token_iter(
        mut iter: Balanced<I>,
    ) -> Result<TokenIter<std_iter::Empty<Token>>, UnmatchedError> {
        let mut all_chunks = Vec::new();
        let mut chunk_start = iter.pos();
        let mut chunk = Vec::new();

        loop {
            match iter.next() {
                Some(Ok(Backslash)) => {
                    let next_pos = iter.pos();
                    match iter.next() {
                        Some(Ok(tok @ Dollar))
                        | Some(Ok(tok @ Backtick))
                        | Some(Ok(tok @ Backslash)) => {
                            all_chunks.push((chunk, chunk_start));
                            chunk_start = next_pos;
                            chunk = vec![tok];
                        }

                        Some(tok) => {
                            chunk.push(Backslash);
                            chunk.push(tok?);
                        }

                        None => chunk.push(Backslash),
                    }
                }

                Some(tok) => chunk.push(tok?),
                None => break,
            }
        }

        if !chunk.is_empty() {
            all_chunks.push((chunk, chunk_start));
        }

        let mut tok_iter = TokenIter::with_position(std_iter::empty(), iter.pos());
        while let Some((chunk, chunk_end)) = all_chunks.pop() {
            tok_iter.buffer_tokens_to_yield_first(chunk, chunk_end);
        }
        Ok(tok_iter)
    }
}

impl<I> std::iter::FusedIterator for BacktickBackslashRemover<I> where
    I: PeekablePositionIterator<Item = Token>
{
}

impl<I: PeekablePositionIterator<Item = Token>> Iterator for BacktickBackslashRemover<I> {
    type Item = Result<Token, UnmatchedError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.peeked.is_some() {
            return self.peeked.take();
        } else if self.done {
            return None;
        }

        match self.iter.next() {
            Some(Ok(Backslash)) => match self.iter.next() {
                ret @ Some(Ok(Dollar)) | ret @ Some(Ok(Backtick)) | ret @ Some(Ok(Backslash)) => {
                    ret
                }

                Some(t) => {
                    debug_assert!(self.peeked.is_none());
                    self.peeked = Some(t);
                    Some(Ok(Backslash))
                }

                None => {
                    self.done = true;
                    Some(Ok(Backslash))
                }
            },

            Some(t) => Some(t),
            None => {
                self.done = true;
                None
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // The number of tokens we actually yield will never be
        // more than those of the underlying iterator, and will
        // probably be less, but this is a good enough estimate.
        self.iter.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::{PositionIterator, TokenIter, TokenOrPos};
    use crate::parse::SourcePos;
    use crate::token::Token;

    #[test]
    fn test_multipeek() {
        let tokens = vec![
            Token::ParenOpen,
            Token::Semi,
            Token::Dollar,
            Token::ParenClose,
        ];

        let mut tok_iter = TokenIter::new(tokens.clone().into_iter());
        {
            let mut multipeek = tok_iter.multipeek();
            let mut expected_peeked = tokens.iter();
            while let Some(t) = multipeek.peek_next() {
                assert_eq!(expected_peeked.next(), Some(t));
            }

            // Exhausted the expected stream
            assert_eq!(expected_peeked.next(), None);
        }

        // Original iterator still yields the expected values
        assert_eq!(tokens, tok_iter.collect::<Vec<_>>());
    }

    #[test]
    fn test_buffering_tokens_should_immediately_update_position() {
        fn src(byte: usize, line: usize, col: usize) -> SourcePos {
            SourcePos { byte, line, col }
        }

        let mut tok_iter = TokenIter::new(std::iter::empty());

        let pos = src(4, 4, 4);

        tok_iter.buffer_tokens_and_positions_to_yield_first(
            vec![
                TokenOrPos::Pos(src(2, 2, 2)),
                TokenOrPos::Pos(src(3, 3, 3)),
                TokenOrPos::Pos(pos),
                TokenOrPos::Tok(Token::Newline),
            ],
            Some(src(1, 1, 1)),
        );

        assert_eq!(tok_iter.pos(), pos);
    }
}
