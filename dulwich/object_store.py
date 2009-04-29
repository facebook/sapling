# object_store.py -- Object store for git objects 
# Copyright (C) 2008 Jelmer Vernooij <jelmer@samba.org>
# 
# This program is free software; you can redistribute it and/or
# modify it under the terms of the GNU General Public License
# as published by the Free Software Foundation; either version 2
# or (at your option) a later version of the License.
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
import tempfile
import urllib2

from errors import (
    NotTreeError,
    )
from objects import (
    ShaFile,
    Tree,
    hex_to_sha,
    sha_to_hex,
    )
from pack import (
    Pack,
    PackData, 
    iter_sha1, 
    load_packs, 
    load_pack_index,
    write_pack,
    write_pack_data,
    write_pack_index_v2,
    )

PACKDIR = 'pack'

class ObjectStore(object):
    """Object store."""

    def __init__(self, path):
        """Open an object store.

        :param path: Path of the object store.
        """
        self.path = path
        self._pack_cache = None
        self.pack_dir = os.path.join(self.path, PACKDIR)

    def determine_wants_all(self, refs):
	    return [sha for (ref, sha) in refs.iteritems() if not sha in self and not ref.endswith("^{}")]

    def iter_shas(self, shas):
        """Iterate over the objects for the specified shas.

        :param shas: Iterable object with SHAs
        """
        return ObjectStoreIterator(self, shas)

    def __contains__(self, sha):
        for pack in self.packs:
            if sha in pack:
                return True
        ret = self._get_shafile(sha)
        if ret is not None:
            return True
        return False

    @property
    def packs(self):
        """List with pack objects."""
        if self._pack_cache is None:
            self._pack_cache = list(load_packs(self.pack_dir))
        return self._pack_cache

    def _add_known_pack(self, path):
        """Add a newly appeared pack to the cache by path.

        """
        if self._pack_cache is not None:
            self._pack_cache.append(Pack(path))

    def _get_shafile_path(self, sha):
        dir = sha[:2]
        file = sha[2:]
        # Check from object dir
        return os.path.join(self.path, dir, file)

    def _get_shafile(self, sha):
        path = self._get_shafile_path(sha)
        if os.path.exists(path):
          return ShaFile.from_file(path)
        return None

    def _add_shafile(self, sha, o):
        dir = os.path.join(self.path, sha[:2])
        if not os.path.isdir(dir):
            os.mkdir(dir)
        path = os.path.join(dir, sha[2:])
        f = open(path, 'w+')
        try:
            f.write(o.as_legacy_object())
        finally:
            f.close()

    def get_raw(self, name):
        """Obtain the raw text for an object.
        
        :param name: sha for the object.
        :return: tuple with object type and object contents.
        """
        if len(name) == 40:
            sha = hex_to_sha(name)
            hexsha = name
        elif len(name) == 20:
            sha = name
            hexsha = None
        else:
            raise AssertionError
        for pack in self.packs:
            try:
                return pack.get_raw(sha)
            except KeyError:
                pass
        if hexsha is None: 
            hexsha = sha_to_hex(name)
        ret = self._get_shafile(hexsha)
        if ret is not None:
            return ret.as_raw_string()
        raise KeyError(hexsha)

    def __getitem__(self, sha):
        type, uncomp = self.get_raw(sha)
        return ShaFile.from_raw_string(type, uncomp)

    def move_in_thin_pack(self, path):
        """Move a specific file containing a pack into the pack directory.

        :note: The file should be on the same file system as the 
            packs directory.

        :param path: Path to the pack file.
        """
        data = PackData(path)

        # Write index for the thin pack (do we really need this?)
        temppath = os.path.join(self.pack_dir, 
            sha_to_hex(urllib2.randombytes(20))+".tempidx")
        data.create_index_v2(temppath, self.get_raw)
        p = Pack.from_objects(data, load_pack_index(temppath))

        # Write a full pack version
        temppath = os.path.join(self.pack_dir, 
            sha_to_hex(urllib2.randombytes(20))+".temppack")
        write_pack(temppath, ((o, None) for o in p.iterobjects(self.get_raw)), 
                len(p))
        pack_sha = load_pack_index(temppath+".idx").objects_sha1()
        newbasename = os.path.join(self.pack_dir, "pack-%s" % pack_sha)
        os.rename(temppath+".pack", newbasename+".pack")
        os.rename(temppath+".idx", newbasename+".idx")
        self._add_known_pack(newbasename)

    def move_in_pack(self, path):
        """Move a specific file containing a pack into the pack directory.

        :note: The file should be on the same file system as the 
            packs directory.

        :param path: Path to the pack file.
        """
        p = PackData(path)
        entries = p.sorted_entries(self.get_raw)
        basename = os.path.join(self.pack_dir, 
            "pack-%s" % iter_sha1(entry[0] for entry in entries))
        write_pack_index_v2(basename+".idx", entries, p.get_stored_checksum())
        os.rename(path, basename + ".pack")
        self._add_known_pack(basename)

    def add_thin_pack(self):
        """Add a new thin pack to this object store.

        Thin packs are packs that contain deltas with parents that exist 
        in a different pack.
        """
        fd, path = tempfile.mkstemp(dir=self.pack_dir, suffix=".pack")
        f = os.fdopen(fd, 'w')
        def commit():
            os.fsync(fd)
            f.close()
            if os.path.getsize(path) > 0:
                self.move_in_thin_pack(path)
        return f, commit

    def add_pack(self):
        """Add a new pack to this object store. 

        :return: Fileobject to write to and a commit function to 
            call when the pack is finished.
        """
        fd, path = tempfile.mkstemp(dir=self.pack_dir, suffix=".pack")
        f = os.fdopen(fd, 'w')
        def commit():
            #os.fsync(fd)
            #f.close()
            if os.path.getsize(path) > 0:
                self.move_in_pack(path)
        return f, commit

    def add_object(self, obj):
        self._add_shafile(obj.id, obj)

    def add_objects(self, objects):
        """Add a set of objects to this object store.

        :param objects: Iterable over a list of objects.
        """
        if len(objects) == 0:
            return
        f, commit = self.add_pack()
        write_pack_data(f, objects, len(objects))
        commit()


class ObjectImporter(object):
    """Interface for importing objects."""

    def __init__(self, count):
        """Create a new ObjectImporter.

        :param count: Number of objects that's going to be imported.
        """
        self.count = count

    def add_object(self, object):
        """Add an object."""
        raise NotImplementedError(self.add_object)

    def finish(self, object):
        """Finish the imoprt and write objects to disk."""
        raise NotImplementedError(self.finish)


class ObjectIterator(object):
    """Interface for iterating over objects."""

    def iterobjects(self):
        raise NotImplementedError(self.iterobjects)


class ObjectStoreIterator(ObjectIterator):
    """ObjectIterator that works on top of an ObjectStore."""

    def __init__(self, store, sha_iter):
        self.store = store
        self.sha_iter = sha_iter
        self._shas = []

    def __iter__(self):
        for sha, path in self.itershas():
            yield self.store[sha], path

    def iterobjects(self):
        for o, path in self:
            yield o

    def itershas(self):
        for sha in self._shas:
            yield sha
        for sha in self.sha_iter:
            self._shas.append(sha)
            yield sha

    def __contains__(self, needle):
        """Check if an object is present.

        :param needle: SHA1 of the object to check for
        """
        return needle in self.store

    def __getitem__(self, key):
        """Find an object by SHA1."""
        return self.store[key]

    def __len__(self):
        """Return the number of objects."""
        return len(list(self.itershas()))


def tree_lookup_path(lookup_obj, root_sha, path):
    parts = path.split("/")
    sha = root_sha
    for p in parts:
        obj = lookup_obj(sha)
        if type(obj) is not Tree:
            raise NotTreeError(sha)
        if p == '':
            continue
        mode, sha = obj[p]
    return lookup_obj(sha)
