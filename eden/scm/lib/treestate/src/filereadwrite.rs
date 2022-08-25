/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::BufWriter;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;

pub trait FileSync {
    fn sync_all(&mut self) -> Result<(), std::io::Error>;
}

pub trait FileReadWrite: std::io::Read + std::io::Write + std::io::Seek + FileSync + Send {}

pub struct FileReaderWriter {
    writer: BufWriter<File>,
}

impl FileReaderWriter {
    pub fn new(writer: BufWriter<File>) -> Self {
        Self { writer }
    }
}

impl Read for FileReaderWriter {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.writer.get_mut().read(buf)
    }
}

impl Write for FileReaderWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl Seek for FileReaderWriter {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.writer.seek(pos)
    }
}

impl FileSync for FileReaderWriter {
    fn sync_all(&mut self) -> Result<(), std::io::Error> {
        self.writer.get_mut().sync_all()
    }
}

impl FileReadWrite for FileReaderWriter {}

impl<T> FileSync for Cursor<T> {
    fn sync_all(&mut self) -> Result<(), std::io::Error> {
        Ok(())
    }
}

impl<T> FileReadWrite for Cursor<T>
where
    Cursor<T>: Write,
    T: std::convert::AsRef<[u8]> + Send,
{
}
