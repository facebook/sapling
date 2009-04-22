# test_protocol.py -- Tests for the git protocol
# Copyright (C) 2009 Jelmer Vernooij <jelmer@samba.org>
# 
# This program is free software; you can redistribute it and/or
# modify it under the terms of the GNU General Public License
# as published by the Free Software Foundation; version 2
# or (at your option) any later version of the License.
# 
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
# 
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston,
# MA  02110-1301, USA.

from cStringIO import StringIO
from unittest import TestCase

from dulwich.protocol import (
    Protocol,
    extract_capabilities,
    )

class ProtocolTests(TestCase):

    def setUp(self):
        self.rout = StringIO()
        self.rin = StringIO()
        self.proto = Protocol(self.rin.read, self.rout.write)

    def test_write_pkt_line_none(self):
        self.proto.write_pkt_line(None)
        self.assertEquals(self.rout.getvalue(), "0000")

    def test_write_pkt_line(self):
        self.proto.write_pkt_line("bla")
        self.assertEquals(self.rout.getvalue(), "0007bla")

    def test_read_pkt_line(self):
        self.rin.write("0008cmd ")
        self.rin.seek(0)
        self.assertEquals("cmd ", self.proto.read_pkt_line())

    def test_read_pkt_seq(self):
        self.rin.write("0008cmd 0005l0000")
        self.rin.seek(0)
        self.assertEquals(["cmd ", "l"], list(self.proto.read_pkt_seq()))

    def test_read_pkt_line_none(self):
        self.rin.write("0000")
        self.rin.seek(0)
        self.assertEquals(None, self.proto.read_pkt_line())

    def test_write_sideband(self):
        self.proto.write_sideband(3, "bloe")
        self.assertEquals(self.rout.getvalue(), "0009\x03bloe")

    def test_send_cmd(self):
        self.proto.send_cmd("fetch", "a", "b")
        self.assertEquals(self.rout.getvalue(), "000efetch a\x00b\x00")

    def test_read_cmd(self):
        self.rin.write("0012cmd arg1\x00arg2\x00")
        self.rin.seek(0)
        self.assertEquals(("cmd", ["arg1", "arg2"]), self.proto.read_cmd())

    def test_read_cmd_noend0(self):
        self.rin.write("0011cmd arg1\x00arg2")
        self.rin.seek(0)
        self.assertRaises(AssertionError, self.proto.read_cmd)


class ExtractCapabilitiesTestCase(TestCase):

    def test_plain(self):
        self.assertEquals(("bla", None), extract_capabilities("bla"))

    def test_caps(self):
        self.assertEquals(("bla", ["la", "la"]), extract_capabilities("bla\0la\0la"))
