/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use packetline::encode::write_binary_packetline;
use packetline::encode::write_text_packetline;
use pin_project::pin_project;
use tokio::io::AsyncWrite;
use tokio::pin;
use tokio::time;
use tokio::time::Duration;
use tokio::time::Sleep;

#[pin_project]
pub struct TestAsyncWriter {
    #[pin]
    pub state: Vec<u8>,
    sleep_future: Pin<Box<Sleep>>,
    random_counter: u32,
}

impl TestAsyncWriter {
    pub fn new() -> Self {
        Self {
            state: vec![],
            random_counter: 0,
            sleep_future: Box::pin(time::sleep(Duration::from_millis(100))),
        }
    }

    pub fn contents(&self) -> String {
        String::from_utf8(self.state.clone()).unwrap()
    }
}

impl AsyncWrite for TestAsyncWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.project();
        *this.random_counter += 1;
        // Return poll pending for every third write attempt
        if *this.random_counter % 2 == 0 {
            let sleep = this.sleep_future.as_mut();
            if sleep.poll(cx).is_pending() {
                return Poll::Pending;
            } else {
                *this.sleep_future = Box::pin(time::sleep(Duration::from_millis(100)));
            }
        }
        this.state.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.project();
        this.state.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.project();
        this.state.poll_shutdown(cx)
    }
}

#[fbinit::test]
async fn validate_packetline_writer_basic_case() -> anyhow::Result<()> {
    let mut writer = TestAsyncWriter::new();
    write_binary_packetline(b"Hello this is just a test", &mut writer).await?;
    assert_eq!(
        writer.contents(),
        "001dHello this is just a test".to_string()
    );
    Ok(())
}

#[fbinit::test]
async fn validate_packetline_text_writer_basic_case() -> anyhow::Result<()> {
    let mut writer = TestAsyncWriter::new();
    write_text_packetline(b"Hello this is just a test", &mut writer).await?;
    assert_eq!(
        writer.contents(),
        "001eHello this is just a test\n".to_string()
    );
    Ok(())
}

#[fbinit::test]
async fn validate_packetline_writer_large_write() -> anyhow::Result<()> {
    let mut writer = TestAsyncWriter::new();
    let data = [
        "This function will attempt to write the entire contents of buf, but the entire write may not succeed",
        "or the write may also generate",
        "If the return value is Ok(n) then it must be guaranteed that n <= buf.len(). A return value of 0 typically means that the underlying object is no longer able to accept bytes and will likely not be able to in the future as well, or that",
        "the buffered data cannot be sent until the underlying object is ready to accept more bytes.",
        "This method is cancellation safe in the sense that if it is used as the event in a tokio::select! statement and some other branch",
        "Like write, except that it writes from a slice of buffers.",
        "Writes a buffer into this writer, advancing the buffer's internal cursor.",
        "Each call to write may generate an I/O error indicating that the operation could not be completed. If an error is returned then no bytes in the buffer were written to this writer.",
        "A subsequent call to write_buf using the same buf value will resume from the point that the first call to write_buf completed",
        "This method will continuously call write until there is no more data to be written",
    ];
    let expected_output = "0068This function will attempt to write the entire contents of buf, but the entire write may not succeed0022or the write may also generate00efIf the return value is Ok(n) then it must be guaranteed that n <= buf.len(). A return value of 0 typically means that the underlying object is no longer able to accept bytes and will likely not be able to in the future as well, or that005fthe buffered data cannot be sent until the underlying object is ready to accept more bytes.0085This method is cancellation safe in the sense that if it is used as the event in a tokio::select! statement and some other branch003eLike write, except that it writes from a slice of buffers.004dWrites a buffer into this writer, advancing the buffer's internal cursor.00b7Each call to write may generate an I/O error indicating that the operation could not be completed. If an error is returned then no bytes in the buffer were written to this writer.0081A subsequent call to write_buf using the same buf value will resume from the point that the first call to write_buf completed0056This method will continuously call write until there is no more data to be written";
    for line in data {
        let mut bytes = line.as_bytes();
        while !bytes.is_empty() {
            let written = write_binary_packetline(bytes, &mut writer).await?;
            bytes = bytes.split_at(written + 1).1;
        }
    }
    assert_eq!(writer.contents(), expected_output.to_string());
    Ok(())
}
