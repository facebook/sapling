# test_index.py -- Tests for the git index cache
# Copyright (C) 2008 Jelmer Vernooij <jelmer@samba.org>
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

import os
from unittest import TestCase

from dulwich.index import (
    Index,
    read_index,
    write_index,
    )

class IndexTestCase(TestCase):

    datadir = os.path.join(os.path.dirname(__file__), 'data/indexes')

    def get_simple_index(self, name):
        return Index(os.path.join(self.datadir, name))


class SimpleIndexTestcase(IndexTestCase):

    def test_len(self):
        self.assertEquals(1, len(self.get_simple_index("index")))

    def test_iter(self):
        self.assertEquals([
            ('bla', (1230680220, 0), (1230680220, 0), 2050, 3761020, 33188, 1000, 1000, 0, '\xe6\x9d\xe2\x9b\xb2\xd1\xd6CK\x8b)\xaewZ\xd8\xc2\xe4\x8cS\x91', 3)
            ], 
                list(self.get_simple_index("index")))

    def test_getitem(self):
        self.assertEquals( ('bla', (1230680220, 0), (1230680220, 0), 2050, 3761020, 33188, 1000, 1000, 0, '\xe6\x9d\xe2\x9b\xb2\xd1\xd6CK\x8b)\xaewZ\xd8\xc2\xe4\x8cS\x91', 3)
            , 
                self.get_simple_index("index")["bla"])


class SimpleIndexWriterTestCase(IndexTestCase):

    def test_simple_write(self):
        entries = [('barbla', (1230680220, 0), (1230680220, 0), 2050, 3761020, 33188, 1000, 1000, 0, '\xe6\x9d\xe2\x9b\xb2\xd1\xd6CK\x8b)\xaewZ\xd8\xc2\xe4\x8cS\x91', 3)]
        x = open('test-simple-write-index', 'w+')
        try:
            write_index(x, entries)
        finally:
            x.close()
        x = open('test-simple-write-index', 'r')
        try:
            self.assertEquals(entries, list(read_index(x)))
        finally:
            x.close()

