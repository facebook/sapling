# pack.py -- For dealing wih packed git objects.
# Copyright (C) 2007 James Westby <jw+debian@jameswestby.net>
# Copryight (C) 2008 Jelmer Vernooij <jelmer@samba.org>
# 
# This program is free software; you can redistribute it and/or
# modify it under the terms of the GNU General Public License
# as published by the Free Software Foundation; version 2
# of the License or (at your option) a later version.
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

"""Classes for dealing with packed git objects.

A pack is a compact representation of a bunch of objects, stored
using deltas where possible.

They have two parts, the pack file, which stores the data, and an index
that tells you where the data is.

To find an object you look in all of the index files 'til you find a
match for the object name. You then use the pointer got from this as
a pointer in to the corresponding packfile.
"""

try:
    from collections import defaultdict
except ImportError:
    from misc import defaultdict

from itertools import chain, imap, izip
import mmap
import os
import struct
try:
    from struct import unpack_from
except ImportError:
    from misc import unpack_from
import sys
import zlib
import difflib

from errors import (
    ApplyDeltaError,
    ChecksumMismatch,
    )
from lru_cache import (
    LRUSizeCache,
    )
from objects import (
    ShaFile,
    hex_to_sha,
    sha_to_hex,
    )
from misc import make_sha

supports_mmap_offset = (sys.version_info[0] >= 3 or
        (sys.version_info[0] == 2 and sys.version_info[1] >= 6))


def take_msb_bytes(map, offset):
    ret = []
    while len(ret) == 0 or ret[-1] & 0x80:
        ret.append(ord(map[offset]))
        offset += 1
    return ret


def read_zlib(data, offset, dec_size):
    obj = zlib.decompressobj()
    ret = []
    fed = 0
    while obj.unused_data == "":
        base = offset+fed
        add = data[base:base+1024]
        if len(add) < 1024:
            add += "Z"
        fed += len(add)
        ret.append(obj.decompress(add))
    x = "".join(ret)
    assert len(x) == dec_size
    comp_len = fed-len(obj.unused_data)
    return x, comp_len


def iter_sha1(iter):
    """Return the hexdigest of the SHA1 over a set of names."""
    sha1 = make_sha()
    for name in iter:
        sha1.update(name)
    return sha1.hexdigest()


def simple_mmap(f, offset, size, access=mmap.ACCESS_READ):
    """Simple wrapper for mmap() which always supports the offset parameter.

    :param f: File object.
    :param offset: Offset in the file, from the beginning of the file.
    :param size: Size of the mmap'ed area
    :param access: Access mechanism.
    :return: MMAP'd area.
    """
    mem = mmap.mmap(f.fileno(), size+offset, access=access)
    return mem, offset


def load_pack_index(filename):
    f = open(filename, 'r')
    if f.read(4) == '\377tOc':
        version = struct.unpack(">L", f.read(4))[0]
        if version == 2:
            f.seek(0)
            return PackIndex2(filename, file=f)
        else:
            raise KeyError("Unknown pack index format %d" % version)
    else:
        f.seek(0)
        return PackIndex1(filename, file=f)


def bisect_find_sha(start, end, sha, unpack_name):
    assert start <= end
    while start <= end:
        i = (start + end)/2
        file_sha = unpack_name(i)
        x = cmp(file_sha, sha)
        if x < 0:
            start = i + 1
        elif x > 0:
            end = i - 1
        else:
            return i
    return None


class PackIndex(object):
    """An index in to a packfile.
  
    Given a sha id of an object a pack index can tell you the location in the
    packfile of that object if it has it.
  
    To do the loop it opens the file, and indexes first 256 4 byte groups
    with the first byte of the sha id. The value in the four byte group indexed
    is the end of the group that shares the same starting byte. Subtract one
    from the starting byte and index again to find the start of the group.
    The values are sorted by sha id within the group, so do the math to find
    the start and end offset and then bisect in to find if the value is present.
    """
  
    def __init__(self, filename, file=None):
        """Create a pack index object.
    
        Provide it with the name of the index file to consider, and it will map
        it whenever required.
        """
        self._filename = filename
        # Take the size now, so it can be checked each time we map the file to
        # ensure that it hasn't changed.
        self._size = os.path.getsize(filename)
        if file is None:
            self._file = open(filename, 'r')
        else:
            self._file = file
        self._contents, map_offset = simple_mmap(self._file, 0, self._size)
        assert map_offset == 0
  
    def __eq__(self, other):
        if not isinstance(other, PackIndex):
            return False
    
        if self._fan_out_table != other._fan_out_table:
            return False
    
        for (name1, _, _), (name2, _, _) in izip(self.iterentries(), other.iterentries()):
            if name1 != name2:
                return False
        return True
  
    def close(self):
        self._file.close()
  
    def __len__(self):
        """Return the number of entries in this pack index."""
        return self._fan_out_table[-1]
  
    def _unpack_entry(self, i):
        """Unpack the i-th entry in the index file.
    
        :return: Tuple with object name (SHA), offset in pack file and 
              CRC32 checksum (if known)."""
        raise NotImplementedError(self._unpack_entry)
  
    def _unpack_name(self, i):
        """Unpack the i-th name from the index file."""
        raise NotImplementedError(self._unpack_name)
  
    def _unpack_offset(self, i):
        """Unpack the i-th object offset from the index file."""
        raise NotImplementedError(self._unpack_offset)

    def _unpack_crc32_checksum(self, i):
        """Unpack the crc32 checksum for the i-th object from the index file."""
        raise NotImplementedError(self._unpack_crc32_checksum)
  
    def __iter__(self):
        return imap(sha_to_hex, self._itersha())
  
    def _itersha(self):
        for i in range(len(self)):
            yield self._unpack_name(i)
  
    def objects_sha1(self):
        """Return the hex SHA1 over all the shas of all objects in this pack.
        
        :note: This is used for the filename of the pack.
        """
        return iter_sha1(self._itersha())
  
    def iterentries(self):
        """Iterate over the entries in this pack index.
       
        Will yield tuples with object name, offset in packfile and crc32 checksum.
        """
        for i in range(len(self)):
            yield self._unpack_entry(i)
  
    def _read_fan_out_table(self, start_offset):
        ret = []
        for i in range(0x100):
            ret.append(struct.unpack(">L", self._contents[start_offset+i*4:start_offset+(i+1)*4])[0])
        return ret
  
    def check(self):
        """Check that the stored checksum matches the actual checksum."""
        return self.calculate_checksum() == self.get_stored_checksum()
  
    def calculate_checksum(self):
        return make_sha(self._contents[:-20]).digest()

    def get_pack_checksum(self):
        """Return the SHA1 checksum stored for the corresponding packfile."""
        return str(self._contents[-40:-20])
  
    def get_stored_checksum(self):
        """Return the SHA1 checksum stored for this index."""
        return str(self._contents[-20:])
  
    def object_index(self, sha):
        """Return the index in to the corresponding packfile for the object.
    
        Given the name of an object it will return the offset that object lives
        at within the corresponding pack file. If the pack file doesn't have the
        object then None will be returned.
        """
        if len(sha) == 40:
            sha = hex_to_sha(sha)
        return self._object_index(sha)
  
    def _object_index(self, sha):
        """See object_index.
        
        :param sha: A *binary* SHA string. (20 characters long)_
        """
        assert len(sha) == 20
        idx = ord(sha[0])
        if idx == 0:
            start = 0
        else:
            start = self._fan_out_table[idx-1]
        end = self._fan_out_table[idx]
        i = bisect_find_sha(start, end, sha, self._unpack_name)
        if i is None:
            raise KeyError(sha)
        return self._unpack_offset(i)
            


class PackIndex1(PackIndex):
    """Version 1 Pack Index."""

    def __init__(self, filename, file=None):
        PackIndex.__init__(self, filename, file)
        self.version = 1
        self._fan_out_table = self._read_fan_out_table(0)

    def _unpack_entry(self, i):
        (offset, name) = unpack_from(">L20s", self._contents, 
            (0x100 * 4) + (i * 24))
        return (name, offset, None)
 
    def _unpack_name(self, i):
        offset = (0x100 * 4) + (i * 24) + 4
        return self._contents[offset:offset+20]
  
    def _unpack_offset(self, i):
        offset = (0x100 * 4) + (i * 24)
        return unpack_from(">L", self._contents, offset)[0]
  
    def _unpack_crc32_checksum(self, i):
        # Not stored in v1 index files
        return None 
  

class PackIndex2(PackIndex):
    """Version 2 Pack Index."""

    def __init__(self, filename, file=None):
        PackIndex.__init__(self, filename, file)
        assert self._contents[:4] == '\377tOc', "Not a v2 pack index file"
        (self.version, ) = unpack_from(">L", self._contents, 4)
        assert self.version == 2, "Version was %d" % self.version
        self._fan_out_table = self._read_fan_out_table(8)
        self._name_table_offset = 8 + 0x100 * 4
        self._crc32_table_offset = self._name_table_offset + 20 * len(self)
        self._pack_offset_table_offset = self._crc32_table_offset + 4 * len(self)

    def _unpack_entry(self, i):
        return (self._unpack_name(i), self._unpack_offset(i), 
                self._unpack_crc32_checksum(i))
 
    def _unpack_name(self, i):
        offset = self._name_table_offset + i * 20
        return self._contents[offset:offset+20]
  
    def _unpack_offset(self, i):
        offset = self._pack_offset_table_offset + i * 4
        return unpack_from(">L", self._contents, offset)[0]
  
    def _unpack_crc32_checksum(self, i):
        return unpack_from(">L", self._contents, 
                          self._crc32_table_offset + i * 4)[0]
  


def read_pack_header(f):
    header = f.read(12)
    assert header[:4] == "PACK"
    (version,) = unpack_from(">L", header, 4)
    assert version in (2, 3), "Version was %d" % version
    (num_objects,) = unpack_from(">L", header, 8)
    return (version, num_objects)


def read_pack_tail(f):
    return (f.read(20),)


def unpack_object(map, offset=0):
    bytes = take_msb_bytes(map, offset)
    type = (bytes[0] >> 4) & 0x07
    size = bytes[0] & 0x0f
    for i, byte in enumerate(bytes[1:]):
        size += (byte & 0x7f) << ((i * 7) + 4)
    raw_base = len(bytes)
    if type == 6: # offset delta
        bytes = take_msb_bytes(map, raw_base + offset)
        assert not (bytes[-1] & 0x80)
        delta_base_offset = bytes[0] & 0x7f
        for byte in bytes[1:]:
            delta_base_offset += 1
            delta_base_offset <<= 7
            delta_base_offset += (byte & 0x7f)
        raw_base+=len(bytes)
        uncomp, comp_len = read_zlib(map, offset + raw_base, size)
        assert size == len(uncomp)
        return type, (delta_base_offset, uncomp), comp_len+raw_base
    elif type == 7: # ref delta
        basename = map[offset+raw_base:offset+raw_base+20]
        uncomp, comp_len = read_zlib(map, offset+raw_base+20, size)
        assert size == len(uncomp)
        return type, (basename, uncomp), comp_len+raw_base+20
    else:
        uncomp, comp_len = read_zlib(map, offset+raw_base, size)
        assert len(uncomp) == size
        return type, uncomp, comp_len+raw_base


def compute_object_size((num, obj)):
    if num in (6, 7):
        return len(obj[1])
    assert isinstance(obj, str)
    return len(obj)


class PackData(object):
    """The data contained in a packfile.
  
    Pack files can be accessed both sequentially for exploding a pack, and
    directly with the help of an index to retrieve a specific object.
  
    The objects within are either complete or a delta aginst another.
  
    The header is variable length. If the MSB of each byte is set then it
    indicates that the subsequent byte is still part of the header.
    For the first byte the next MS bits are the type, which tells you the type
    of object, and whether it is a delta. The LS byte is the lowest bits of the
    size. For each subsequent byte the LS 7 bits are the next MS bits of the
    size, i.e. the last byte of the header contains the MS bits of the size.
  
    For the complete objects the data is stored as zlib deflated data.
    The size in the header is the uncompressed object size, so to uncompress
    you need to just keep feeding data to zlib until you get an object back,
    or it errors on bad data. This is done here by just giving the complete
    buffer from the start of the deflated object on. This is bad, but until I
    get mmap sorted out it will have to do.
  
    Currently there are no integrity checks done. Also no attempt is made to try
    and detect the delta case, or a request for an object at the wrong position.
    It will all just throw a zlib or KeyError.
    """
  
    def __init__(self, filename):
        """Create a PackData object that represents the pack in the given filename.
    
        The file must exist and stay readable until the object is disposed of. It
        must also stay the same size. It will be mapped whenever needed.
    
        Currently there is a restriction on the size of the pack as the python
        mmap implementation is flawed.
        """
        self._filename = filename
        assert os.path.exists(filename), "%s is not a packfile" % filename
        self._size = os.path.getsize(filename)
        self._header_size = 12
        assert self._size >= self._header_size, "%s is too small for a packfile (%d < %d)" % (filename, self._size, self._header_size)
        self._file = open(self._filename, 'rb')
        self._read_header()
        self._offset_cache = LRUSizeCache(1024*1024*20, 
            compute_size=compute_object_size)

    def close(self):
        self._file.close()
  
    def _read_header(self):
        (version, self._num_objects) = read_pack_header(self._file)
        self._file.seek(self._size-20)
        (self._stored_checksum,) = read_pack_tail(self._file)
  
    def __len__(self):
        """Returns the number of objects in this pack."""
        return self._num_objects
  
    def calculate_checksum(self):
        """Calculate the checksum for this pack."""
        map, map_offset = simple_mmap(self._file, 0, self._size - 20)
        try:
            r = make_sha(map[map_offset:self._size-20]).digest()
            map.close()
            return r
        except:
            map.close()
            raise

    def resolve_object(self, offset, type, obj, get_ref, get_offset=None):
        """Resolve an object, possibly resolving deltas when necessary.
        
        :return: Tuple with object type and contents.
        """
        if type not in (6, 7): # Not a delta
            return type, obj

        if get_offset is None:
            get_offset = self.get_object_at
      
        if type == 6: # offset delta
            (delta_offset, delta) = obj
            assert isinstance(delta_offset, int)
            assert isinstance(delta, str)
            base_offset = offset-delta_offset
            type, base_obj = get_offset(base_offset)
            assert isinstance(type, int)
        elif type == 7: # ref delta
            (basename, delta) = obj
            assert isinstance(basename, str) and len(basename) == 20
            assert isinstance(delta, str)
            type, base_obj = get_ref(basename)
            assert isinstance(type, int)
            # Can't be a ofs delta, as we wouldn't know the base offset
            assert type != 6
            base_offset = None
        type, base_text = self.resolve_object(base_offset, type, base_obj, get_ref)
        if base_offset is not None:
            self._offset_cache[base_offset] = type, base_text
        ret = (type, apply_delta(base_text, delta))
        return ret
  
    def iterobjects(self, progress=None):
        offset = self._header_size
        num = len(self)
        map, _ = simple_mmap(self._file, 0, self._size)
        try:
            for i in range(num):
                (type, obj, total_size) = unpack_object(map, offset)
                crc32 = zlib.crc32(map[offset:offset+total_size]) & 0xffffffff
                yield offset, type, obj, crc32
                offset += total_size
                if progress:
                    progress(i, num)
            map.close()
        except:
            map.close()
            raise
  
    def iterentries(self, ext_resolve_ref=None, progress=None):
        found = {}
        postponed = defaultdict(list)
        class Postpone(Exception):
            """Raised to postpone delta resolving."""
        def get_ref_text(sha):
            assert len(sha) == 20
            if sha in found:
                return found[sha]
            if ext_resolve_ref:
                try:
                    return ext_resolve_ref(sha)
                except KeyError:
                    pass
            raise Postpone, (sha, )
        extra = []
        todo = chain(self.iterobjects(progress=progress), extra)
        for (offset, type, obj, crc32) in todo:
            assert isinstance(offset, int)
            assert isinstance(type, int)
            assert isinstance(obj, tuple) or isinstance(obj, str)
            try:
                type, obj = self.resolve_object(offset, type, obj, get_ref_text)
            except Postpone, (sha, ):
                postponed[sha].append((offset, type, obj))
            else:
                shafile = ShaFile.from_raw_string(type, obj)
                sha = shafile.sha().digest()
                found[sha] = (type, obj)
                yield sha, offset, crc32
                extra.extend(postponed.get(sha, []))
        if postponed:
            raise KeyError([sha_to_hex(h) for h in postponed.keys()])
  
    def sorted_entries(self, resolve_ext_ref=None, progress=None):
        ret = list(self.iterentries(resolve_ext_ref, progress=progress))
        ret.sort()
        return ret
  
    def create_index_v1(self, filename, resolve_ext_ref=None, progress=None):
        entries = self.sorted_entries(resolve_ext_ref, progress=progress)
        write_pack_index_v1(filename, entries, self.calculate_checksum())
  
    def create_index_v2(self, filename, resolve_ext_ref=None, progress=None):
        entries = self.sorted_entries(resolve_ext_ref, progress=progress)
        write_pack_index_v2(filename, entries, self.calculate_checksum())
  
    def get_stored_checksum(self):
        return self._stored_checksum
  
    def check(self):
        return (self.calculate_checksum() == self.get_stored_checksum())
  
    def get_object_at(self, offset):
        """Given an offset in to the packfile return the object that is there.
    
        Using the associated index the location of an object can be looked up, and
        then the packfile can be asked directly for that object using this
        function.
        """
        if offset in self._offset_cache:
            return self._offset_cache[offset]
        assert isinstance(offset, long) or isinstance(offset, int),\
                "offset was %r" % offset
        assert offset >= self._header_size
        map, map_offset = simple_mmap(self._file, offset, self._size-offset)
        try:
            ret = unpack_object(map, map_offset)[:2]
            return ret
        finally:
            map.close()


class SHA1Writer(object):
    
    def __init__(self, f):
        self.f = f
        self.sha1 = make_sha("")

    def write(self, data):
        self.sha1.update(data)
        self.f.write(data)

    def write_sha(self):
        sha = self.sha1.digest()
        assert len(sha) == 20
        self.f.write(sha)
        return sha

    def close(self):
        sha = self.write_sha()
        self.f.close()
        return sha

    def tell(self):
        return self.f.tell()


def write_pack_object(f, type, object):
    """Write pack object to a file.

    :param f: File to write to
    :param o: Object to write
    :return: Tuple with offset at which the object was written, and crc32
    """
    ret = f.tell()
    packed_data_hdr = ""
    if type == 6: # ref delta
        (delta_base_offset, object) = object
    elif type == 7: # offset delta
        (basename, object) = object
    size = len(object)
    c = (type << 4) | (size & 15)
    size >>= 4
    while size:
        packed_data_hdr += (chr(c | 0x80))
        c = size & 0x7f
        size >>= 7
    packed_data_hdr += chr(c)
    if type == 6: # offset delta
        ret = [delta_base_offset & 0x7f]
        delta_base_offset >>= 7
        while delta_base_offset:
            delta_base_offset -= 1
            ret.insert(0, 0x80 | (delta_base_offset & 0x7f))
            delta_base_offset >>= 7
        packed_data_hdr += "".join([chr(x) for x in ret])
    elif type == 7: # ref delta
        assert len(basename) == 20
        packed_data_hdr += basename
    packed_data = packed_data_hdr + zlib.compress(object)
    f.write(packed_data)
    return (f.tell(), (zlib.crc32(packed_data) & 0xffffffff))


def write_pack(filename, objects, num_objects):
    f = open(filename + ".pack", 'w')
    try:
        entries, data_sum = write_pack_data(f, objects, num_objects)
    finally:
        f.close()
    entries.sort()
    write_pack_index_v2(filename + ".idx", entries, data_sum)


def write_pack_data(f, objects, num_objects, window=10):
    """Write a new pack file.

    :param filename: The filename of the new pack file.
    :param objects: List of objects to write (tuples with object and path)
    :return: List with (name, offset, crc32 checksum) entries, pack checksum
    """
    recency = list(objects)
    # FIXME: Somehow limit delta depth
    # FIXME: Make thin-pack optional (its not used when cloning a pack)
    # Build a list of objects ordered by the magic Linus heuristic
    # This helps us find good objects to diff against us
    magic = []
    for obj, path in recency:
        magic.append( (obj.type, path, 1, -len(obj.as_raw_string()[1]), obj) )
    magic.sort()
    # Build a map of objects and their index in magic - so we can find preceeding objects
    # to diff against
    offs = {}
    for i in range(len(magic)):
        offs[magic[i][4]] = i
    # Write the pack
    entries = []
    f = SHA1Writer(f)
    f.write("PACK")               # Pack header
    f.write(struct.pack(">L", 2)) # Pack version
    f.write(struct.pack(">L", num_objects)) # Number of objects in pack
    for o, path in recency:
        sha1 = o.sha().digest()
        orig_t, raw = o.as_raw_string()
        winner = raw
        t = orig_t
        #for i in range(offs[o]-window, window):
        #    if i < 0 or i >= len(offs): continue
        #    b = magic[i][4]
        #    if b.type != orig_t: continue
        #    _, base = b.as_raw_string()
        #    delta = create_delta(base, raw)
        #    if len(delta) < len(winner):
        #        winner = delta
        #        t = 6 if magic[i][2] == 1 else 7
        offset, crc32 = write_pack_object(f, t, winner)
        entries.append((sha1, offset, crc32))
    return entries, f.write_sha()


def write_pack_index_v1(filename, entries, pack_checksum):
    """Write a new pack index file.

    :param filename: The filename of the new pack index file.
    :param entries: List of tuples with object name (sha), offset_in_pack,  and
            crc32_checksum.
    :param pack_checksum: Checksum of the pack file.
    """
    f = open(filename, 'w')
    f = SHA1Writer(f)
    fan_out_table = defaultdict(lambda: 0)
    for (name, offset, entry_checksum) in entries:
        fan_out_table[ord(name[0])] += 1
    # Fan-out table
    for i in range(0x100):
        f.write(struct.pack(">L", fan_out_table[i]))
        fan_out_table[i+1] += fan_out_table[i]
    for (name, offset, entry_checksum) in entries:
        f.write(struct.pack(">L20s", offset, name))
    assert len(pack_checksum) == 20
    f.write(pack_checksum)
    f.close()


def create_delta(base_buf, target_buf):
    """Use python difflib to work out how to transform base_buf to target_buf"""
    assert isinstance(base_buf, str)
    assert isinstance(target_buf, str)
    out_buf = ""
    # write delta header
    def encode_size(size):
        ret = ""
        c = size & 0x7f
        size >>= 7
        while size:
            ret += chr(c | 0x80)
            c = size & 0x7f
            size >>= 7
        ret += chr(c)
        return ret
    out_buf += encode_size(len(base_buf))
    out_buf += encode_size(len(target_buf))
    # write out delta opcodes
    seq = difflib.SequenceMatcher(a=base_buf, b=target_buf)
    for opcode, i1, i2, j1, j2 in seq.get_opcodes():
        # Git patch opcodes don't care about deletes!
        #if opcode == "replace" or opcode == "delete":
        #    pass
        if opcode == "equal":
            # If they are equal, unpacker will use data from base_buf
            # Write out an opcode that says what range to use
            scratch = ""
            op = 0x80
            o = i1
            for i in range(4):
                if o & 0xff << i*8:
                    scratch += chr(o >> i)
                    op |= 1 << i
            s = i2 - i1
            for i in range(2):
                if s & 0xff << i*8:
                    scratch += chr(s >> i)
                    op |= 1 << (4+i)
            out_buf += chr(op)
            out_buf += scratch
        if opcode == "replace" or opcode == "insert":
            # If we are replacing a range or adding one, then we just
            # output it to the stream (prefixed by its size)
            s = j2 - j1
            o = j1
            while s > 127:
                out_buf += chr(127)
                out_buf += target_buf[o:o+127]
                s -= 127
                o += 127
            out_buf += chr(s)
            out_buf += target_buf[o:o+s]
    return out_buf


def apply_delta(src_buf, delta):
    """Based on the similar function in git's patch-delta.c.
    
    :param src_buf: Source buffer
    :param delta: Delta instructions
    """
    assert isinstance(src_buf, str), "was %r" % (src_buf,)
    assert isinstance(delta, str)
    out = []
    index = 0
    delta_length = len(delta)
    def get_delta_header_size(delta, index):
        size = 0
        i = 0
        while delta:
            cmd = ord(delta[index])
            index += 1
            size |= (cmd & ~0x80) << i
            i += 7
            if not cmd & 0x80:
                break
        return size, index
    src_size, index = get_delta_header_size(delta, index)
    dest_size, index = get_delta_header_size(delta, index)
    assert src_size == len(src_buf), "%d vs %d" % (src_size, len(src_buf))
    while index < delta_length:
        cmd = ord(delta[index])
        index += 1
        if cmd & 0x80:
            cp_off = 0
            for i in range(4):
                if cmd & (1 << i): 
                    x = ord(delta[index])
                    index += 1
                    cp_off |= x << (i * 8)
            cp_size = 0
            for i in range(3):
                if cmd & (1 << (4+i)): 
                    x = ord(delta[index])
                    index += 1
                    cp_size |= x << (i * 8)
            if cp_size == 0: 
                cp_size = 0x10000
            if (cp_off + cp_size < cp_size or
                cp_off + cp_size > src_size or
                cp_size > dest_size):
                break
            out.append(src_buf[cp_off:cp_off+cp_size])
        elif cmd != 0:
            out.append(delta[index:index+cmd])
            index += cmd
        else:
            raise ApplyDeltaError("Invalid opcode 0")
    
    if index != delta_length:
        raise ApplyDeltaError("delta not empty: %r" % delta[index:])

    out = ''.join(out)
    if dest_size != len(out):
        raise ApplyDeltaError("dest size incorrect")

    return out


def write_pack_index_v2(filename, entries, pack_checksum):
    """Write a new pack index file.

    :param filename: The filename of the new pack index file.
    :param entries: List of tuples with object name (sha), offset_in_pack,  and
            crc32_checksum.
    :param pack_checksum: Checksum of the pack file.
    """
    f = open(filename, 'w')
    f = SHA1Writer(f)
    f.write('\377tOc') # Magic!
    f.write(struct.pack(">L", 2))
    fan_out_table = defaultdict(lambda: 0)
    for (name, offset, entry_checksum) in entries:
        fan_out_table[ord(name[0])] += 1
    # Fan-out table
    for i in range(0x100):
        f.write(struct.pack(">L", fan_out_table[i]))
        fan_out_table[i+1] += fan_out_table[i]
    for (name, offset, entry_checksum) in entries:
        f.write(name)
    for (name, offset, entry_checksum) in entries:
        f.write(struct.pack(">L", entry_checksum))
    for (name, offset, entry_checksum) in entries:
        # FIXME: handle if MSBit is set in offset
        f.write(struct.pack(">L", offset))
    # FIXME: handle table for pack files > 8 Gb
    assert len(pack_checksum) == 20
    f.write(pack_checksum)
    f.close()


class Pack(object):

    def __init__(self, basename):
        self._basename = basename
        self._data_path = self._basename + ".pack"
        self._idx_path = self._basename + ".idx"
        self._data = None
        self._idx = None

    @classmethod
    def from_objects(self, data, idx):
        ret = Pack("")
        ret._data = data
        ret._idx = idx
        return ret

    def name(self):
        """The SHA over the SHAs of the objects in this pack."""
        return self.idx.objects_sha1()

    @property
    def data(self):
        if self._data is None:
            self._data = PackData(self._data_path)
            assert len(self.idx) == len(self._data)
            idx_stored_checksum = self.idx.get_pack_checksum()
            data_stored_checksum = self._data.get_stored_checksum()
            if idx_stored_checksum != data_stored_checksum:
                raise ChecksumMismatch(sha_to_hex(idx_stored_checksum), 
                                       sha_to_hex(data_stored_checksum))
        return self._data

    @property
    def idx(self):
        if self._idx is None:
            self._idx = load_pack_index(self._idx_path)
        return self._idx

    def close(self):
        if self._data is not None:
            self._data.close()
        self.idx.close()

    def __eq__(self, other):
        return type(self) == type(other) and self.idx == other.idx

    def __len__(self):
        """Number of entries in this pack."""
        return len(self.idx)

    def __repr__(self):
        return "%s(%r)" % (self.__class__.__name__, self._basename)

    def __iter__(self):
        """Iterate over all the sha1s of the objects in this pack."""
        return iter(self.idx)

    def check(self):
        if not self.idx.check():
            return False
        if not self.data.check():
            return False
        return True

    def get_stored_checksum(self):
        return self.data.get_stored_checksum()

    def __contains__(self, sha1):
        """Check whether this pack contains a particular SHA1."""
        try:
            self.idx.object_index(sha1)
            return True
        except KeyError:
            return False

    def get_raw(self, sha1, resolve_ref=None):
        offset = self.idx.object_index(sha1)
        obj_type, obj = self.data.get_object_at(offset)
        if type(offset) is long:
          offset = int(offset)
        if resolve_ref is None:
            resolve_ref = self.get_raw
        return self.data.resolve_object(offset, obj_type, obj, resolve_ref)

    def __getitem__(self, sha1):
        """Retrieve the specified SHA1."""
        type, uncomp = self.get_raw(sha1)
        return ShaFile.from_raw_string(type, uncomp)

    def iterobjects(self, get_raw=None):
        if get_raw is None:
            get_raw = self.get_raw
        for offset, type, obj, crc32 in self.data.iterobjects():
            assert isinstance(offset, int)
            yield ShaFile.from_raw_string(
                    *self.data.resolve_object(offset, type, obj, get_raw))


def load_packs(path):
    if not os.path.exists(path):
        return
    for name in os.listdir(path):
        if name.startswith("pack-") and name.endswith(".pack"):
            yield Pack(os.path.join(path, name[:-len(".pack")]))


try:
    from _pack import apply_delta, bisect_find_sha
except ImportError:
    pass
