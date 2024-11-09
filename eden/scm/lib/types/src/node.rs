/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
