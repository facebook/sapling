# pointer.py - Git-LFS pointer serialization
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import re

from mercurial.i18n import _

from mercurial import (
    error,
)

class InvalidPointer(error.RevlogError):
    pass

class gitlfspointer(dict):
    VERSION = 'https://git-lfs.github.com/spec/v1'

    def __init__(self, *args, **kwargs):
        self['version'] = self.VERSION
        super(gitlfspointer, self).__init__(*args, **kwargs)

    @classmethod
    def deserialize(cls, text):
        try:
            return cls(l.split(' ', 1) for l in text.splitlines()).validate()
        except ValueError: # l.split returns 1 item instead of 2
            raise InvalidPointer(_('cannot parse git-lfs text: %r') % text)

    def serialize(self):
        sortkeyfunc = lambda x: (x[0] != 'version', x)
        items = sorted(self.validate().iteritems(), key=sortkeyfunc)
        return ''.join('%s %s\n' % (k, v) for k, v in items)

    def oid(self):
        return self['oid'].split(':')[-1]

    def size(self):
        return int(self['size'])

    # regular expressions used by _validate
    # see https://github.com/git-lfs/git-lfs/blob/master/docs/spec.md
    _keyre = re.compile(r'\A[a-z0-9.-]+\Z')
    _valuere = re.compile(r'\A[^\n]*\Z')
    _requiredre = {
        'size': re.compile(r'\A[0-9]+\Z'),
        'oid': re.compile(r'\Asha256:[0-9a-f]{64}\Z'),
        'version': re.compile(r'\A%s\Z' % re.escape(VERSION)),
    }

    def validate(self):
        """raise InvalidPointer on error. return self if there is no error"""
        requiredcount = 0
        for k, v in self.iteritems():
            if k in self._requiredre:
                if not self._requiredre[k].match(v):
                    raise InvalidPointer(_('unexpected value: %s=%r') % (k, v))
                requiredcount += 1
            elif not self._keyre.match(k):
                raise InvalidPointer(_('unexpected key: %s') % k)
            if not self._valuere.match(v):
                raise InvalidPointer(_('unexpected value: %s=%r') % (k, v))
        if len(self._requiredre) != requiredcount:
            miss = sorted(set(self._requiredre.keys()).difference(self.keys()))
            raise InvalidPointer(_('missed keys: %s') % ', '.join(miss))
        return self

deserialize = gitlfspointer.deserialize
