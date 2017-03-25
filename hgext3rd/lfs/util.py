# coding=UTF-8

from __future__ import absolute_import

import hashlib

from mercurial import (
    revlog,
    util,
)

safehasattr = util.safehasattr

def sha256(text):
    digest = hashlib.sha256()
    digest.update(text)
    return digest.hexdigest()

def hash(text, p1, p2):
    return revlog.hash(text, p1, p2)

def getoption(opener, option):
    options = getattr(opener, 'options', None)
    if options:
        return options.get(option)
    return None
