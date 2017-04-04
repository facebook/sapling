# coding=UTF-8

from __future__ import absolute_import

import hashlib
import re

from mercurial import (
    error,
    util,
    vfs as vfsmod,
)

safehasattr = util.safehasattr

def sha256(text):
    digest = hashlib.sha256()
    digest.update(text)
    return digest.hexdigest()

# 40 bytes for SHA1, 64 bytes for SHA256
_lfsre = re.compile(r'\A[a-f0-9]{40,64}\Z')

class lfsvfs(vfsmod.vfs):
    def join(self, path):
        """split the path at first two characters, like: XX/XXXXX..."""
        if not _lfsre.match(path):
            raise error.ProgrammingError('unexpected lfs path: %s' % path)
        return super(lfsvfs, self).join(path[0:2], path[2:])
