# index.py -- File parser/write for the git index file
# Copyright (C) 2008-2009 Jelmer Vernooij <jelmer@samba.org>
 
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

import os
import stat
import struct

from dulwich.objects import (
    Tree,
    hex_to_sha,
    sha_to_hex,
    )


def read_cache_time(f):
    """Read a cache time."""
    return struct.unpack(">LL", f.read(8))


def write_cache_time(f, t):
    """Write a cache time."""
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
    return (name, ctime, mtime, ino, dev, mode, uid, gid, size, 
            sha_to_hex(sha), flags)


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
    f.write(struct.pack(">LLLLLL20sH", ino, dev, mode, uid, gid, size, hex_to_sha(sha), flags))
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


def cleanup_mode(mode):
    if stat.S_ISLNK(fsmode):
        mode = stat.S_IFLNK
    else:
        mode = stat.S_IFREG
    mode |= (fsmode & 0111)
    return mode


class Index(object):
    """A Git Index file."""

    def __init__(self, filename):
        """Open an index file.
        
        :param filename: Path to the index file
        """
        self._filename = filename
        self.clear()
        self.read()

    def write(self):
        """Write current contents of index to disk."""
        f = open(self._filename, 'w')
        try:
            write_index_dict(f, self._byname)
        finally:
            f.close()

    def read(self):
        """Read current contents of index from disk."""
        f = open(self._filename, 'r')
        try:
            for x in read_index(f):
                self[x[0]] = tuple(x[1:])
        finally:
            f.close()

    def __len__(self):
        """Number of entries in this index file."""
        return len(self._byname)

    def __getitem__(self, name):
        """Retrieve entry by relative path."""
        return self._byname[name]

    def __iter__(self):
        """Iterate over the paths in this index."""
        return iter(self._byname)

    def get_sha1(self, path):
        """Return the (git object) SHA1 for the object at a path."""
        return self[path][-2]

    def iterblobs(self):
        """Iterate over path, sha, mode tuples for use with commit_tree."""
        for path, entry in self:
            yield path, entry[-2], cleanup_mode(entry[-6])

    def clear(self):
        """Remove all contents from this index."""
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


def commit_tree(object_store, blobs):
    """Commit a new tree.

    :param object_store: Object store to add trees to
    :param blobs: Iterable over blob path, sha, mode entries
    :return: SHA1 of the created tree.
    """
    trees = {"": {}}
    def add_tree(path):
        if path in trees:
            return trees[path]
        dirname, basename = os.path.split(path)
        t = add_tree(dirname)
        assert isinstance(basename, str)
        newtree = {}
        t[basename] = newtree
        trees[path] = newtree
        return newtree

    for path, sha, mode in blobs:
        tree_path, basename = os.path.split(path)
        tree = add_tree(tree_path)
        tree[basename] = (mode, sha)

    def build_tree(path):
        tree = Tree()
        for basename, entry in trees[path].iteritems():
            if type(entry) == dict:
                mode = stat.S_IFDIR
                sha = build_tree(os.path.join(path, basename))
            else:
                (mode, sha) = entry
            tree.add(mode, basename, sha)
        object_store.add_object(tree)
        return tree.id
    return build_tree("")


def commit_index(object_store, index):
    return commit_tree(object_store, index.blobs())
