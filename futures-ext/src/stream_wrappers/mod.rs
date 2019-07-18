// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub mod collect_no_consume;
pub mod collect_to;
pub mod take_while;

use futures::Stream;

pub use self::collect_no_consume::CollectNoConsume;
pub use self::collect_to::CollectTo;
pub use self::take_while::TakeWhile;

/// A stream that wraps another stream. into_inner consumes this stream and
/// returns the original stream.
pub trait StreamWrapper<S: Stream> {
    // TODO: this trait might not be necessary once
    // https://github.com/alexcrichton/futures-rs/pull/523 lands
    fn into_inner(self) -> S;
}

/// A stream that wraps another stream. into_inner consumes this stream and
/// returns the original stream.
///
/// This is a boxed version of StreamWrapper, meant to be used for trait
/// objects.
pub trait BoxStreamWrapper<S: Stream> {
    fn get_ref(&self) -> &S;
    fn get_mut(&mut self) -> &mut S;
    fn into_inner(self: Box<Self>) -> S;
}
