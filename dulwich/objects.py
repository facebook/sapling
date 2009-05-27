# objects.py -- Access to base git objects
# Copyright (C) 2007 James Westby <jw+debian@jameswestby.net>
# Copyright (C) 2008-2009 Jelmer Vernooij <jelmer@samba.org>
# 
# This program is free software; you can redistribute it and/or
# modify it under the terms of the GNU General Public License
# as published by the Free Software Foundation; version 2
# of the License or (at your option) a later version of the License.
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


"""Access to base git objects."""


import mmap
import os
import sha
import stat
import zlib

from errors import (
    NotBlobError,
    NotCommitError,
    NotTreeError,
    )
from misc import make_sha

BLOB_ID = "blob"
TAG_ID = "tag"
TREE_ID = "tree"
COMMIT_ID = "commit"
PARENT_ID = "parent"
AUTHOR_ID = "author"
COMMITTER_ID = "committer"
ENCODING_ID = "encoding"
OBJECT_ID = "object"
TYPE_ID = "type"
TAGGER_ID = "tagger"

def _decompress(string):
    dcomp = zlib.decompressobj()
    dcomped = dcomp.decompress(string)
    dcomped += dcomp.flush()
    return dcomped


# SC hacked this to keep a global dict of already hexed shas because the
# import script calls this a bajillion times.  Will try to cache other areas
# so this isn't called as much in the first place.
already_hexed_shas = {}
def sha_to_hex(sha):
    """Takes a string and returns the hex of the sha within"""
    if sha in already_hexed_shas:
        return already_hexed_shas[sha]
    hexsha = "".join(["%02x" % ord(c) for c in sha])
    assert len(hexsha) == 40, "Incorrect length of sha1 string: %d" % hexsha
    already_hexed_shas[sha] = hexsha
    return hexsha


def hex_to_sha(hex):
    """Takes a hex sha and returns a binary sha"""
    assert len(hex) == 40, "Incorrent length of hexsha: %s" % hex
    return ''.join([chr(int(hex[i:i+2], 16)) for i in xrange(0, len(hex), 2)])


def serializable_property(name, docstring=None):
    def set(obj, value):
        obj._ensure_parsed()
        setattr(obj, "_"+name, value)
        obj._needs_serialization = True
    def get(obj):
        obj._ensure_parsed()
        return getattr(obj, "_"+name)
    return property(get, set, doc=docstring)


class ShaFile(object):
    """A git SHA file."""
  
    @classmethod
    def _parse_legacy_object(cls, map):
        """Parse a legacy object, creating it and setting object._text"""
        text = _decompress(map)
        object = None
        for posstype in type_map.keys():
            if text.startswith(posstype):
                object = type_map[posstype]()
                text = text[len(posstype):]
                break
        assert object is not None, "%s is not a known object type" % text[:9]
        assert text[0] == ' ', "%s is not a space" % text[0]
        text = text[1:]
        size = 0
        i = 0
        while text[0] >= '0' and text[0] <= '9':
            if i > 0 and size == 0:
                assert False, "Size is not in canonical format"
            size = (size * 10) + int(text[0])
            text = text[1:]
            i += 1
        object._size = size
        assert text[0] == "\0", "Size not followed by null"
        text = text[1:]
        object.set_raw_string(text)
        return object

    def as_legacy_object(self):
        text = self.as_raw_string()
        return zlib.compress("%s %d\0%s" % (self._type, len(text), text))
  
    def as_raw_string(self):
        if self._needs_serialization:
            self.serialize()
        return self._text

    def as_pretty_string(self):
        return self.as_raw_string()

    def _ensure_parsed(self):
        if self._needs_parsing:
            self._parse_text()

    def set_raw_string(self, text):
        self._text = text
        self._sha = None
        self._needs_parsing = True
        self._needs_serialization = False
  
    @classmethod
    def _parse_object(cls, map):
        """Parse a new style object , creating it and setting object._text"""
        used = 0
        byte = ord(map[used])
        used += 1
        num_type = (byte >> 4) & 7
        try:
            object = num_type_map[num_type]()
        except KeyError:
            raise AssertionError("Not a known type: %d" % num_type)
        while (byte & 0x80) != 0:
            byte = ord(map[used])
            used += 1
        raw = map[used:]
        object.set_raw_string(_decompress(raw))
        return object
  
    @classmethod
    def _parse_file(cls, map):
        word = (ord(map[0]) << 8) + ord(map[1])
        if ord(map[0]) == 0x78 and (word % 31) == 0:
            return cls._parse_legacy_object(map)
        else:
            return cls._parse_object(map)
  
    def __init__(self):
        """Don't call this directly"""
        self._sha = None
  
    def _parse_text(self):
        """For subclasses to do initialisation time parsing"""
  
    @classmethod
    def from_file(cls, filename):
        """Get the contents of a SHA file on disk"""
        size = os.path.getsize(filename)
        f = open(filename, 'rb')
        try:
            map = mmap.mmap(f.fileno(), size, access=mmap.ACCESS_READ)
            shafile = cls._parse_file(map)
            return shafile
        finally:
            f.close()
  
    @classmethod
    def from_raw_string(cls, type, string):
        """Creates an object of the indicated type from the raw string given.
    
        Type is the numeric type of an object. String is the raw uncompressed
        contents.
        """
        real_class = num_type_map[type]
        obj = real_class()
        obj.type = type
        obj.set_raw_string(string)
        return obj
  
    def _header(self):
        return "%s %lu\0" % (self._type, len(self.as_raw_string()))
  
    def sha(self):
        """The SHA1 object that is the name of this object."""
        if self._needs_serialization or self._sha is None:
            self._sha = make_sha()
            self._sha.update(self._header())
            self._sha.update(self.as_raw_string())
        return self._sha
  
    @property
    def id(self):
        return self.sha().hexdigest()
  
    def get_type(self):
        return self._num_type

    def set_type(self, type):
        self._num_type = type

    type = property(get_type, set_type)
  
    def __repr__(self):
        return "<%s %s>" % (self.__class__.__name__, self.id)
  
    def __eq__(self, other):
        """Return true id the sha of the two objects match.
  
        The __le__ etc methods aren't overriden as they make no sense,
        certainly at this level.
        """
        return self.sha().digest() == other.sha().digest()


class Blob(ShaFile):
    """A Git Blob object."""

    _type = BLOB_ID
    _num_type = 3
    _needs_serialization = False
    _needs_parsing = False

    def get_data(self):
        return self._text

    def set_data(self, data):
        self._text = data

    data = property(get_data, set_data, 
            "The text contained within the blob object.")

    @classmethod
    def from_file(cls, filename):
        blob = ShaFile.from_file(filename)
        if blob._type != cls._type:
            raise NotBlobError(filename)
        return blob

    @classmethod
    def from_string(cls, string):
        """Create a blob from a string."""
        shafile = cls()
        shafile.set_raw_string(string)
        return shafile


class Tag(ShaFile):
    """A Git Tag object."""

    _type = TAG_ID
    _num_type = 4

    @classmethod
    def from_file(cls, filename):
        blob = ShaFile.from_file(filename)
        if blob._type != cls._type:
            raise NotBlobError(filename)
        return blob

    @classmethod
    def from_string(cls, string):
        """Create a blob from a string."""
        shafile = cls()
        shafile.set_raw_string(string)
        return shafile

    def _parse_text(self):
        """Grab the metadata attached to the tag"""
        text = self._text
        count = 0
        assert text.startswith(OBJECT_ID), "Invalid tag object, " \
            "must start with %s" % OBJECT_ID
        count += len(OBJECT_ID)
        assert text[count] == ' ', "Invalid tag object, " \
            "%s must be followed by space not %s" % (OBJECT_ID, text[count])
        count += 1
        self._object_sha = text[count:count+40]
        count += 40
        assert text[count] == '\n', "Invalid tag object, " \
            "%s sha must be followed by newline" % OBJECT_ID
        count += 1
        assert text[count:].startswith(TYPE_ID), "Invalid tag object, " \
            "%s sha must be followed by %s" % (OBJECT_ID, TYPE_ID)
        count += len(TYPE_ID)
        assert text[count] == ' ', "Invalid tag object, " \
            "%s must be followed by space not %s" % (TAG_ID, text[count])
        count += 1
        self._object_type = ""
        while text[count] != '\n':
            self._object_type += text[count]
            count += 1
        count += 1
        assert self._object_type in (COMMIT_ID, BLOB_ID, TREE_ID, TAG_ID), "Invalid tag object, " \
            "unexpected object type %s" % self._object_type
        self._object_type = type_map[self._object_type]

        assert text[count:].startswith(TAG_ID), "Invalid tag object, " \
            "object type must be followed by %s" % (TAG_ID)
        count += len(TAG_ID)
        assert text[count] == ' ', "Invalid tag object, " \
            "%s must be followed by space not %s" % (TAG_ID, text[count])
        count += 1
        self._name = ""
        while text[count] != '\n':
            self._name += text[count]
            count += 1
        count += 1

        assert text[count:].startswith(TAGGER_ID), "Invalid tag object, " \
            "%s must be followed by %s" % (TAG_ID, TAGGER_ID)
        count += len(TAGGER_ID)
        assert text[count] == ' ', "Invalid tag object, " \
            "%s must be followed by space not %s" % (TAGGER_ID, text[count])
        count += 1
        self._tagger = ""
        while text[count] != '>':
            assert text[count] != '\n', "Malformed tagger information"
            self._tagger += text[count]
            count += 1
        self._tagger += text[count]
        count += 1
        assert text[count] == ' ', "Invalid tag object, " \
            "tagger information must be followed by space not %s" % text[count]
        count += 1
        self._tag_time = int(text[count:count+10])
        while text[count] != '\n':
            count += 1
        count += 1
        assert text[count] == '\n', "There must be a new line after the headers"
        count += 1
        self._message = text[count:]
        self._needs_parsing = False

    def get_object(self):
        """Returns the object pointed by this tag, represented as a tuple(type, sha)"""
        self._ensure_parsed()
        return (self._object_type, self._object_sha)

    object = property(get_object)

    name = serializable_property("name", "The name of this tag")
    tagger = serializable_property("tagger", 
        "Returns the name of the person who created this tag")
    tag_time = serializable_property("tag_time", 
        "The creation timestamp of the tag.  As the number of seconds since the epoch")
    message = serializable_property("message", "The message attached to this tag")


def parse_tree(text):
    ret = {}
    count = 0
    while count < len(text):
        mode = 0
        chr = text[count]
        while chr != ' ':
            assert chr >= '0' and chr <= '7', "%s is not a valid mode char" % chr
            mode = (mode << 3) + (ord(chr) - ord('0'))
            count += 1
            chr = text[count]
        count += 1
        chr = text[count]
        name = ''
        while chr != '\0':
            name += chr
            count += 1
            chr = text[count]
        count += 1
        chr = text[count]
        sha = text[count:count+20]
        hexsha = sha_to_hex(sha)
        ret[name] = (mode, hexsha)
        count = count + 20
    return ret


class Tree(ShaFile):
    """A Git tree object"""

    _type = TREE_ID
    _num_type = 2

    def __init__(self):
        super(Tree, self).__init__()
        self._entries = {}
        self._needs_parsing = False
        self._needs_serialization = True

    @classmethod
    def from_file(cls, filename):
        tree = ShaFile.from_file(filename)
        if tree._type != cls._type:
            raise NotTreeError(filename)
        return tree

    def __contains__(self, name):
        self._ensure_parsed()
        return name in self._entries

    def __getitem__(self, name):
        self._ensure_parsed()
        return self._entries[name]

    def __setitem__(self, name, value):
        assert isinstance(value, tuple)
        assert len(value) == 2
        self._ensure_parsed()
        self._entries[name] = value
        self._needs_serialization = True

    def __delitem__(self, name):
        self._ensure_parsed()
        del self._entries[name]
        self._needs_serialization = True

    def entry(self, name):
        self._ensure_parsed()
        try:
            return self._entries[name]
        except:
            return (None, None)
        
    def add(self, mode, name, hexsha):
        assert type(mode) == int
        assert type(name) == str
        assert type(hexsha) == str
        self._ensure_parsed()
        self._entries[name] = mode, hexsha
        self._needs_serialization = True

    def entries(self):
        """Return a list of tuples describing the tree entries"""
        self._ensure_parsed()
        # The order of this is different from iteritems() for historical reasons
        return [(mode, name, hexsha) for (name, mode, hexsha) in self.iteritems()]

    def iteritems(self):
        self._ensure_parsed()
        for name in sorted(self._entries.keys()):
            yield name, self._entries[name][0], self._entries[name][1]

    def _parse_text(self):
        """Grab the entries in the tree"""
        self._entries = parse_tree(self._text)
        self._needs_parsing = False

    def serialize(self):
        self._text = ""
        for name, mode, hexsha in self.iteritems():
            self._text += "%04o %s\0%s" % (mode, name, hex_to_sha(hexsha))
        self._needs_serialization = False

    def as_pretty_string(self):
        text = ""
        for name, mode, hexsha in self.iteritems():
            if mode & stat.S_IFDIR:
                kind = "tree"
            else:
                kind = "blob"
            text += "%04o %s %s\t%s\n" % (mode, kind, hexsha, name)
        return text


def parse_timezone(text):
    offset = int(text)
    signum = (offset < 0) and -1 or 1
    offset = abs(offset)
    hours = int(offset / 100)
    minutes = (offset % 100)
    return signum * (hours * 3600 + minutes * 60)


def format_timezone(offset):
    if offset % 60 != 0:
        raise ValueError("Unable to handle non-minute offset.")
    sign = (offset < 0) and '-' or '+'
    offset = abs(offset)
    return '%c%02d%02d' % (sign, offset / 3600, (offset / 60) % 60)


class Commit(ShaFile):
    """A git commit object"""

    _type = COMMIT_ID
    _num_type = 1

    def __init__(self):
        super(Commit, self).__init__()
        self._parents = []
        self._needs_parsing = False
        self._needs_serialization = True

    @classmethod
    def from_file(cls, filename):
        commit = ShaFile.from_file(filename)
        if commit._type != cls._type:
            raise NotCommitError(filename)
        return commit

    def _parse_text(self):
        text = self._text
        count = 0
        assert text.startswith(TREE_ID), "Invalid commit object, " \
             "must start with %s" % TREE_ID
        count += len(TREE_ID)
        assert text[count] == ' ', "Invalid commit object, " \
             "%s must be followed by space not %s" % (TREE_ID, text[count])
        count += 1
        self._tree = text[count:count+40]
        count = count + 40
        assert text[count] == "\n", "Invalid commit object, " \
             "tree sha must be followed by newline"
        count += 1
        self._parents = []
        while text[count:].startswith(PARENT_ID):
            count += len(PARENT_ID)
            assert text[count] == ' ', "Invalid commit object, " \
                 "%s must be followed by space not %s" % (PARENT_ID, text[count])
            count += 1
            self._parents.append(text[count:count+40])
            count += 40
            assert text[count] == "\n", "Invalid commit object, " \
                 "parent sha must be followed by newline"
            count += 1
        self._author = None
        if text[count:].startswith(AUTHOR_ID):
            count += len(AUTHOR_ID)
            assert text[count] == ' ', "Invalid commit object, " \
                 "%s must be followed by space not %s" % (AUTHOR_ID, text[count])
            count += 1
            self._author = ''
            while text[count] != '>':
                assert text[count] != '\n', "Malformed author information"
                self._author += text[count]
                count += 1
            self._author += text[count]
            self._author_raw = self._author
            count += 1
            assert text[count] == ' ', "Invalid commit object, " \
                 "author information must be followed by space not %s" % text[count]
            count += 1
            self._author_raw += ' ' + text[count:].split(" ", 1)[0]
            self._author_time = int(text[count:].split(" ", 1)[0])
            while text[count] != ' ':
                assert text[count] != '\n', "Malformed author information"
                count += 1
            self._author_raw += text[count:count+6]
            self._author_timezone = parse_timezone(text[count:count+6])
            count += 1
            while text[count] != '\n':
                count += 1
            count += 1
        self._committer = None
        if text[count:].startswith(COMMITTER_ID):
            count += len(COMMITTER_ID)
            assert text[count] == ' ', "Invalid commit object, " \
                 "%s must be followed by space not %s" % (COMMITTER_ID, text[count])
            count += 1
            self._committer = ''
            while text[count] != '>':
                assert text[count] != '\n', "Malformed committer information"
                self._committer += text[count]
                count += 1
            self._committer += text[count]
            self._committer_raw = self._committer
            count += 1
            assert text[count] == ' ', "Invalid commit object, " \
                 "commiter information must be followed by space not %s" % text[count]
            count += 1
            self._committer_raw += ' ' + text[count:].split(" ", 1)[0]
            self._commit_time = int(text[count:].split(" ", 1)[0])
            while text[count] != ' ':
                assert text[count] != '\n', "Malformed committer information"
                count += 1
            self._committer_raw += text[count:count+6]
            self._commit_timezone = parse_timezone(text[count:count+6])
            count += 1
            while text[count] != '\n':
                count += 1
            count += 1
        self._encoding = None
        if not text[count] == '\n':
            # There can be an encoding field.
            if text[count:].startswith(ENCODING_ID):
                count += len(ENCODING_ID)
                assert text[count] == ' ', "Invalid encoding, " \
                     "%s must be followed by space not %s" % (ENCODING_ID, text[count])
                count += 1
                self._encoding = text[count:].split("\n", 1)[0]
                while text[count] != "\n":
                    count += 1
        count += 1
        self._message = text[count:]
        self._needs_parsing = False

    def serialize(self):
        self._text = ""
        self._text += "%s %s\n" % (TREE_ID, self._tree)
        for p in self._parents:
            self._text += "%s %s\n" % (PARENT_ID, p)
        self._text += "%s %s %s %s\n" % (AUTHOR_ID, self._author, str(self._author_time), format_timezone(self._author_timezone))
        self._text += "%s %s %s %s\n" % (COMMITTER_ID, self._committer, str(self._commit_time), format_timezone(self._commit_timezone))
        self._text += "\n" # There must be a new line after the headers
        self._text += self._message
        self._needs_serialization = False

    tree = serializable_property("tree", "Tree that is the state of this commit")

    def get_parents(self):
        """Return a list of parents of this commit."""
        self._ensure_parsed()
        return self._parents

    def set_parents(self, value):
        """Return a list of parents of this commit."""
        self._ensure_parsed()
        self._needs_serialization = True
        self._parents = value

    parents = property(get_parents, set_parents)

    author = serializable_property("author", 
        "The name of the author of the commit")

    committer = serializable_property("committer", 
        "The name of the committer of the commit")

    message = serializable_property("message",
        "The commit message")

    commit_time = serializable_property("commit_time",
        "The timestamp of the commit. As the number of seconds since the epoch.")

    commit_timezone = serializable_property("commit_timezone",
        "The zone the commit time is in")

    author_time = serializable_property("author_time", 
        "The timestamp the commit was written. as the number of seconds since the epoch.")

    author_timezone = serializable_property("author_timezone", 
        "Returns the zone the author time is in.")


type_map = {
    BLOB_ID : Blob,
    TREE_ID : Tree,
    COMMIT_ID : Commit,
    TAG_ID: Tag,
}

num_type_map = {
    0: None,
    1: Commit,
    2: Tree,
    3: Blob,
    4: Tag,
    # 5 Is reserved for further expansion
}

try:
    # Try to import C versions
    from _objects import hex_to_sha, sha_to_hex
except ImportError:
    pass

