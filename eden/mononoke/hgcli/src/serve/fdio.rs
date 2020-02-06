/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Provide access blocking IO sources/sinks asynchronously
//!
//! Adapter for blocking IO source (implements std::io::Read) or
//! sink (implements std::io::Write) to an async Stream or Sink.
//! The async portion is implemented with a `futures::sync::mpsc`
//! bounded channel pair.
use std::io::{self, Read, Write};
use std::thread;

use bytes::Bytes;
use futures::sync::mpsc::{channel, Receiver, Sender};
use futures::{Future, Sink, Stream};
use futures_ext::{BoxStream, StreamExt};

const BUFSZ: usize = 8192;
const NUMBUFS: usize = 50000;

/// Async adapter for `Read`
#[derive(Debug)]
pub struct Reader<R> {
    reader: R,
}

impl<R> Reader<R>
where
    R: Read + Send + 'static,
{
    /// Construct an adapter for a synchronous source.
    pub fn new(reader: R) -> Self {
        Reader { reader }
    }

    /// Return an async Stream containing chunks of data read from the
    /// `reader`. Each chunk is an instance of `std::io::Result`, but it will shut
    /// down after sending the first error encountered.
    ///
    /// This method starts a thread to manage the synchronous source. The thread will
    /// shut down once a read completes, either because it returned an error or because
    /// the channel shuts down.
    ///
    /// The two parameters are the number of queued buffers and the max buffer size.
    /// The buffer size should match the expected IO size; if its much larger then it
    /// could result in wasted memory - especially if there are a lot of queued buffers.
    /// The number of queued buffers governs the amount of coupling between the IO source
    /// and its consumer.
    ///
    /// The bufsz must be > 0; it will panic if it is zero.
    pub fn source(self, nbuffers: usize, bufsz: usize) -> Receiver<io::Result<Bytes>> {
        let (tx, rx) = channel(nbuffers);
        let mut reader = self.reader;

        if bufsz == 0 {
            panic!("Must have non-zero buffer size")
        }

        thread::spawn(move || {
            let mut tx = tx;
            loop {
                let mut buf = vec![0; bufsz];

                // Fill the buffer, and trim it to the size we actually filled
                let r = match reader.read(&mut buf[..]) {
                    Ok(0) => break, // EOF
                    Ok(sz) => {
                        buf.truncate(sz);
                        Ok(Bytes::from(buf))
                    }
                    Err(e) => Err(e),
                };

                // The send consumes the result, so remember if it was an error
                let iserr = r.is_err();

                // Send the result. This synchronously waits for the send to complete.
                tx = match tx.send(r).wait() {
                    Ok(tx) => tx,
                    Err(_) => break, // send failed - probably the Receiver was closed
                };

                // We're done
                if iserr {
                    break;
                }
            }
        });

        rx
    }
}

/// Async adapter for `Write`
#[derive(Debug)]
pub struct Writer<W> {
    writer: W,
}

impl<W> Writer<W>
where
    W: Write + Send + 'static,
{
    /// Construct an adapter for a synchronous sink.
    pub fn new(writer: W) -> Self {
        Writer { writer }
    }

    /// Construct an async Sink
    ///
    /// The `nbuffers` parameter specifies how many buffers are queued before
    /// blocking; this defines the amount of coupling there is between the data
    /// producer and its sink.
    ///
    /// This method creates a thread to manage the synchronous sink. The thread
    /// exits if either the `Sender` is closed or if the write fails. There's no
    /// path to return a write failure, so the sender will simply start to get
    /// errors.
    ///
    // TODO(jsgf): return a second Future with results?
    pub fn sink(self, nbuffers: usize) -> Sender<Bytes> {
        let (tx, rx) = channel::<Bytes>(nbuffers);
        let mut writer = self.writer;

        thread::spawn(move || {
            // Block on the `Receiver` waiting for new things to write.
            for buf in rx.wait().map(Result::unwrap) {
                if writer.write_all(&buf[..]).is_err() {
                    break;
                };
                // The writer might be buffering internally, especially if
                // there's no newline at the end of buf.
                if writer.flush().is_err() {
                    break;
                }
            }
        });

        tx
    }
}

/// Helper to produce an async stream for stdin
pub fn stdin() -> BoxStream<Bytes, io::Error> {
    Reader::new(io::stdin())
        .source(NUMBUFS, BUFSZ)
        .then(Result::unwrap)
        .boxify()
}

/// Helper to produce an async sink for stdout
pub fn stdout() -> Sender<Bytes> {
    Writer::new(io::stdout()).sink(NUMBUFS)
}

/// Helper to produce an async sink for stderr
pub fn stderr() -> Sender<Bytes> {
    Writer::new(io::stderr()).sink(NUMBUFS)
}
