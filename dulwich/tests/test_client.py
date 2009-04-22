# test_client.py -- Tests for the git protocol, client side
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

from client import (
    GitClient,
    )

class GitClientTests(TestCase):

    def setUp(self):
        self.rout = StringIO()
        self.rin = StringIO()
        self.client = GitClient(lambda x: True, self.rin.read, 
            self.rout.write)

    def test_caps(self):
        self.assertEquals(['multi_ack', 'side-band-64k', 'ofs-delta', 'thin-pack'], self.client._capabilities)

    def test_fetch_pack_none(self):
        self.rin.write(
            "008855dcc6bf963f922e1ed5c4bbaaefcfacef57b1d7 HEAD.multi_ack thin-pack side-band side-band-64k ofs-delta shallow no-progress include-tag\n"
            "0000")
        self.rin.seek(0)
        self.client.fetch_pack("bla", lambda heads: [], None, None, None)
        self.assertEquals(self.rout.getvalue(), "0000")
