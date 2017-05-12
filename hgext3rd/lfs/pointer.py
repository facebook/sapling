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

class GithubPointer(dict):
    VERSION = 'https://git-lfs.github.com/spec/v1'

    def __init__(self, *args, **kwargs):
        self['version'] = self.VERSION
        super(GithubPointer, self).__init__(*args, **kwargs)

    @classmethod
    def deserialize(cls, text):
        try:
            return cls(l.split(' ', 1) for l in text.splitlines())
        except ValueError: # l.split returns 1 item instead of 2
            raise PointerDeserializationError()

    def serialize(self):
        sortkeyfunc = lambda x: (x[0] != 'version', x)
        items = sorted(self.iteritems(), key=sortkeyfunc)
        return ''.join('%s %s\n' % (k, v) for k, v in items)

    def tostoreid(self):
        return StoreID(self['oid'].split(':')[-1], self['size'])

deserialize = GithubPointer.deserialize
