# coding=UTF-8
from __future__ import absolute_import

import re

from mercurial import (
    error,
)
from mercurial.i18n import _

from .blobstore import StoreID

class PointerDeserializationError(error.RevlogError):
    def __init__(self):
        message = _('invalid lfs pointer format detected')
        super(PointerDeserializationError, self).__init__(message)

class BasePointer(object):

    def __init__(self, extrameta=None):
        self.__metadata = dict()
        if extrameta:
            self.__metadata.update(extrameta)

    def __str__(self):
        return self.serialize()

    def __getitem__(self, key):
        return self.__metadata.get(key)

    def __setitem__(self, key, value):
        self.__metadata[key] = value

    def __contains__(self, key):
        return key in self.__metadata

    def _transformkv(self, key, value):
        return '%s %s\n' % (key, value)

    def keys(self):
        return self.__metadata.keys()

    def serialize(self):
        matcher = re.compile('[a-z0-9\-\.]+')
        text = 'version ' + self.VERSION
        keys = sorted(self.__metadata.keys())
        for key in keys:
            if key == 'version':
                continue
            assert matcher.match(key)
            text = text + self._transformkv(key, self.__metadata[key])
        return text

class GithubPointer(BasePointer):

    VERSION = 'https://git-lfs.github.com/spec/v1\n'

    def __init__(self, oid, hashalgo, size, extrameta=None):
        super(GithubPointer, self).__init__(extrameta)
        self['oid'] = oid
        self['hashalgo'] = hashalgo
        self['size'] = size

    def _transformkv(self, key, value):
        if key == 'hashalgo':
            return ''
        elif key == 'oid':
            return 'oid %s:%s\n' % (self['hashalgo'], value)
        return '%s %s\n' % (key, value)

    @staticmethod
    def deserialize(text):
        metadata = dict()
        for line in text.splitlines()[1:]:
            if len(line) == 0:
                continue
            key, value = line.split(' ', 1)
            if key == 'oid':
                hashalgo, oid = value.split(':', 1)
                metadata['oid'] = str(oid)
                metadata['hashalgo'] = hashalgo
            else:
                metadata[key] = value
        assert 'oid' in metadata
        assert 'size' in metadata
        return GithubPointer(
            oid=metadata['oid'],
            hashalgo=metadata['hashalgo'],
            size=metadata['size'],
            extrameta=metadata)

    def tostoreids(self):
        return [StoreID(self['oid'], self['size'])]

class ChunkingPointer(BasePointer):

    VERSION = 'https://git-lfs.github.com/spec/chunking\n'

    def __init__(self, chunks, hashalgo, size, extrameta=None):
        super(ChunkingPointer, self).__init__(extrameta)
        self['chunks'] = chunks
        self['hashalgo'] = hashalgo
        self['size'] = size

    @staticmethod
    def deserialize(text):
        metadata = dict()
        for line in text.splitlines()[1:]:
            if len(line) == 0:
                continue
            key, value = line.split(' ', 1)
            if key == 'chunks':
                rawchunks = value.split(',')
                chunks = []
                for chunk in rawchunks:
                    oid, size = chunk.split(':', 1)
                    chunks.append({
                        'oid': oid,
                        'size': size
                    })
                metadata['chunks'] = chunks
            else:
                metadata[key] = value
        assert 'chunks' in metadata
        assert 'size' in metadata
        assert 'hashalgo' in metadata
        return ChunkingPointer(
            chunks=metadata['chunks'],
            hashalgo=metadata['hashalgo'],
            size=metadata['size'],
            extrameta=metadata)

    @staticmethod
    def _transformkv(key, value):
        if key == 'chunks':
            rawchunks = ['%s:%s' % (v['oid'], v['size']) for v in value]
            return 'chunks %s\n' % (','.join(rawchunks))
        return '%s %s\n' % (key, value)

    def tostoreids(self):
        return [StoreID(v['oid'], v['size']) for v in self['chunks']]

def deserialize(text):
    pointerformats = [
        GithubPointer,
        ChunkingPointer,
    ]

    for cls in pointerformats:
        if text.startswith('version %s' % cls.VERSION):
            obj = cls.deserialize(text)
            return obj

    raise PointerDeserializationError()
