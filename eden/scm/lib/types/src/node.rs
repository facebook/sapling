/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::io::Read;
use std::io::Write;

use crate::hgid::HgId;
use crate::hgid::ReadHgIdExt;
use crate::hgid::WriteHgIdExt;

pub type Node = HgId;

pub trait WriteNodeExt {
    fn write_node(&mut self, value: &Node) -> io::Result<()>;
}

impl<W: Write + ?Sized> WriteNodeExt for W {
    fn write_node(&mut self, value: &Node) -> io::Result<()> {
        self.write_hgid(value)
    }
}

pub trait ReadNodeExt {
    fn read_node(&mut self) -> io::Result<Node>;
}

impl<R: Read + ?Sized> ReadNodeExt for R {
    fn read_node(&mut self) -> io::Result<Node> {
        self.read_hgid()
    }
}
