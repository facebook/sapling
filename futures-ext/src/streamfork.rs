// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::sink::Sink;
use futures::stream::{Fuse, Stream};
use futures::{Async, AsyncSink, Future, Poll};

/// Fork a Stream into two
///
/// Returns a Future for a process that consumes items from a Stream and
/// forwards them to two sinks depending on a predicate. If the predicate
/// returns false, send the value to out1, otherwise out2.
pub fn streamfork<In, Out1, Out2, F, E>(
    inp: In,
    out1: Out1,
    out2: Out2,
    pred: F,
) -> Forker<In, Out1, Out2, F>
where
    In: Stream,
    Out1: Sink<SinkItem = In::Item>,
    Out2: Sink<SinkItem = In::Item, SinkError = Out1::SinkError>,
    F: FnMut(&In::Item) -> Result<bool, E>,
    E: From<In::Error> + From<Out1::SinkError> + From<Out2::SinkError>,
{
    Forker {
        inp: Some(inp.fuse()),
        out1: Out::new(out1),
        out2: Out::new(out2),
        pred,
    }
}

pub struct Forker<In, Out1, Out2, F>
where
    In: Stream,
    Out1: Sink,
    Out2: Sink,
{
    inp: Option<Fuse<In>>,
    out1: Out<Out1>,
    out2: Out<Out2>,
    pred: F,
}

struct Out<O>
where
    O: Sink,
{
    out: Option<O>,
    buf: Option<O::SinkItem>,
}

impl<S: Sink> Out<S> {
    fn new(s: S) -> Self {
        Out {
            out: Some(s),
            buf: None,
        }
    }

    fn out_mut(&mut self) -> &mut S {
        self.out.as_mut().take().expect("Out after completion")
    }

    fn take_result(&mut self) -> S {
        self.out.take().expect("Out missing")
    }

    fn try_start_send(&mut self, item: S::SinkItem) -> Poll<(), S::SinkError> {
        debug_assert!(self.buf.is_none());

        if let AsyncSink::NotReady(item) = self.out_mut().start_send(item)? {
            self.buf = Some(item);
            return Ok(Async::NotReady);
        }
        Ok(Async::Ready(()))
    }

    fn push(&mut self) -> Poll<(), S::SinkError> {
        if let Some(item) = self.buf.take() {
            self.try_start_send(item)
        } else {
            Ok(Async::Ready(()))
        }
    }

    fn poll_complete(&mut self) -> Poll<(), S::SinkError> {
        self.out_mut().poll_complete()
    }
}

impl<In, Out1, Out2, F> Forker<In, Out1, Out2, F>
where
    In: Stream,
    Out1: Sink,
    Out2: Sink,
{
    fn inp_mut(&mut self) -> &mut Fuse<In> {
        self.inp.as_mut().take().expect("Input after completion")
    }

    fn take_result(&mut self) -> (In, Out1, Out2) {
        let inp = self.inp.take().expect("Input missing in result");
        let out1 = self.out1.take_result();
        let out2 = self.out2.take_result();

        (inp.into_inner(), out1, out2)
    }
}

impl<In, Out1, Out2, F, E> Future for Forker<In, Out1, Out2, F>
where
    In: Stream,
    Out1: Sink<SinkItem = In::Item>,
    Out2: Sink<SinkItem = In::Item>,
    F: FnMut(&In::Item) -> Result<bool, E>,
    E: From<In::Error> + From<Out1::SinkError> + From<Out2::SinkError>,
{
    type Item = (In, Out1, Out2);
    type Error = E;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        // Make sure both outputs are clear to accept new data
        {
            let r1 = self.out1.push()?.is_ready();
            let r2 = self.out2.push()?.is_ready();

            if !(r1 && r2) {
                return Ok(Async::NotReady);
            }
        }

        // Read input and send to outputs until either input dries up or outputs are full
        loop {
            match self.inp_mut().poll()? {
                Async::Ready(Some(item)) => {
                    if (self.pred)(&item)? {
                        try_ready!(self.out2.try_start_send(item))
                    } else {
                        try_ready!(self.out1.try_start_send(item))
                    }
                }
                Async::Ready(None) => {
                    let r1 = self.out1.poll_complete()?.is_ready();
                    let r2 = self.out2.poll_complete()?.is_ready();
                    if !(r1 && r2) {
                        return Ok(Async::NotReady);
                    }
                    return Ok(Async::Ready(self.take_result()));
                }
                Async::NotReady => {
                    self.out1.poll_complete()?;
                    self.out2.poll_complete()?;
                    return Ok(Async::NotReady);
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::stream::iter_ok;
    use futures::Future;

    #[test]
    fn simple() {
        let even = Vec::new();
        let odd = Vec::new();

        let nums = iter_ok((0i32..10).into_iter());
        let (_, even, odd) = streamfork(nums, even, odd, |n| Ok::<_, ()>(*n % 2 == 1))
            .wait()
            .unwrap();

        println!("even={:?}", even);
        println!("odd={:?}", odd);

        assert_eq!(even, vec![0, 2, 4, 6, 8]);
        assert_eq!(odd, vec![1, 3, 5, 7, 9]);
    }
}
