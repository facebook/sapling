# index.py -- File parser/write for the git index file
# Copryight (C) 2008 Jelmer Vernooij <jelmer@samba.org>
 
# This program is free software; you can redistribute it and/or
# modify it under the terms of the GNU General Public License
# as published by the Free Software Foundation; version 2
# of the License or (at your opinion) any later version of the license.
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

"""Parser for the git index file format."""

import struct

def read_cache_time(f):
    return struct.unpack(">LL", f.read(8))


def write_cache_time(f, t):
    if isinstance(t, int):
        t = (t, 0)
    f.write(struct.pack(">LL", *t))


def read_cache_entry(f):
    """Read an entry from a cache file.

    :param f: File-like object to read from
    :return: tuple with: inode, device, mode, uid, gid, size, sha, flags
    """
    beginoffset = f.tell()
    ctime = read_cache_time(f)
    mtime = read_cache_time(f)
    (ino, dev, mode, uid, gid, size, sha, flags, ) = \
        struct.unpack(">LLLLLL20sH", f.read(20 + 4 * 6 + 2))
    name = ""
    char = f.read(1)
    while char != "\0":
        name += char
        char = f.read(1)
    # Padding:
    real_size = ((f.tell() - beginoffset + 7) & ~7)
    f.seek(beginoffset + real_size)
    return (name, ctime, mtime, ino, dev, mode, uid, gid, size, sha, flags)


def write_cache_entry(f, entry):
    """Write an index entry to a file.

    :param f: File object
    :param entry: Entry to write, tuple with: 
        (name, ctime, mtime, ino, dev, mode, uid, gid, size, sha, flags)
    """
    beginoffset = f.tell()
    (name, ctime, mtime, ino, dev, mode, uid, gid, size, sha, flags) = entry
    write_cache_time(f, ctime)
    write_cache_time(f, mtime)
    f.write(struct.pack(">LLLLLL20sH", ino, dev, mode, uid, gid, size, sha, flags))
    f.write(name)
    f.write(chr(0))
    real_size = ((f.tell() - beginoffset + 7) & ~7)
    f.write("\0" * ((beginoffset + real_size) - f.tell()))


def read_index(f):
    """Read an index file, yielding the individual entries."""
    header = f.read(4)
    if header != "DIRC":
        raise AssertionError("Invalid index file header: %r" % header)
    (version, num_entries) = struct.unpack(">LL", f.read(4 * 2))
    assert version in (1, 2)
    for i in range(num_entries):
        yield read_cache_entry(f)


def read_index_dict(f):
    """Read an index file and return it as a dictionary.
    
    :param f: File object to read from
    """
    ret = {}
    for x in read_index(f):
        ret[x[0]] = tuple(x[1:])
    return ret


def write_index(f, entries):
    """Write an index file.
    
    :param f: File-like object to write to
    :param entries: Iterable over the entries to write
    """
    f.write("DIRC")
    f.write(struct.pack(">LL", 2, len(entries)))
    for x in entries:
        write_cache_entry(f, x)


def write_index_dict(f, entries):
    """Write an index file based on the contents of a dictionary.

    """
    entries_list = []
    for name in sorted(entries):
        entries_list.append((name,) + tuple(entries[name]))
    write_index(f, entries_list)


class Index(object):

    def __init__(self, filename):
        self._filename = filename
        self.clear()
        self.read()

    def write(self):
        f = open(self._filename, 'w')
        try:
            write_index_dict(f, self._byname)
        finally:
            f.close()

    def read(self):
        f = open(self._filename, 'r')
        try:
            for x in read_index(f):

                self[x[0]] = tuple(x[1:])
        finally:
            f.close()

    def __len__(self):
        return len(self._byname)

    def __getitem__(self, name):
        return self._byname[name]

    def get_sha1(self, path):
        return self[path][-2]

    def clear(self):
        self._byname = {}

    def __setitem__(self, name, x):
        assert isinstance(name, str)
        assert len(x) == 10
        # Remove the old entry if any
        self._byname[name] = x

    def iteritems(self):
        return self._byname.iteritems()

    def update(self, entries):
        for name, value in entries.iteritems():
            self[name] = value
