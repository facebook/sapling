// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// NOTE: This FileStream wrapper is a bit of a hack while we have an API server that builds on top
// of a BlobRepo abstraction that flattens streams. Indeed, in the API server, we need to know if a
// file is going to exist before we create a response using its stream of bytes. Otherwise, when we
// tell Actix to serve a response using that stream, we don't know if the file exists, so we tell
// Actix to serve up a 200. When Actix tries reading the stream to send it to the client, it gets a
// "not found" error, but by then it's too late to serve a 404 to the client, and Actix just closes
// the connection (it doesn't send _anything_ back, in fact). So, we "fix" that by requiring that
// binary responses have to be created from a FileStream, and the only way to create a FileStream
// is to give it a Stream, and doing that will poll for the first element of the stream to see if
// it's an error (which effectively undoes the flattening BlobRepo did). This is meant to be a
// temporary fix while we work out a better API for getting file streams out of Blobrepo.
//
// If you'd like to refactor this, then the right way to test your fix is to ask a streaming
// endpoint for something that doesn't exist. If you get a 404, you succeeded, congratulations! If
// the connection is closed, or you get a 500, try again :(

use bytes::Bytes;
use failure::Error;
use futures::{
    stream::{iter_ok, once},
    Future, Stream,
};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use mercurial_types::FileBytes;

pub struct FileStream(BoxStream<Bytes, Error>);

impl FileStream {
    pub fn into_bytes_stream(self) -> BoxStream<Bytes, Error> {
        self.0
    }
}

pub trait IntoFileStream {
    fn into_filestream(self) -> BoxFuture<FileStream, Error>;
}

impl<S> IntoFileStream for S
where
    S: Stream<Item = FileBytes, Error = Error> + Send + 'static,
{
    fn into_filestream(self) -> BoxFuture<FileStream, Error> {
        self.map(FileBytes::into_bytes)
            .into_future()
            .map_err(|(err, _stream)| err)
            .map(|(bytes, stream)| {
                let stream = match bytes {
                    Some(bytes) => once(Ok(bytes)).chain(stream).left_stream(),
                    None => iter_ok(vec![]).right_stream(),
                };
                FileStream(stream.boxify())
            })
            .boxify()
    }
}
