// Copyright Facebook, Inc. 2018

use std::io::{self, Read, Write};

use crate::hgid::{HgId, ReadHgIdExt, WriteHgIdExt};

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
