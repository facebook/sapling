# morecolors.py
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""make more output colorful

Currently only ui.traceback is colorized by this extension.
"""

from __future__ import absolute_import

import os
import sys

from mercurial import (
    extensions,
)

colortable = {
    'traceback.foreign': 'red bold',
    'traceback.core': '',
}

def _colorizetraceback(ui, trace):
    state = 'core'
    result = ''
    corepath = os.path.dirname(os.path.dirname(extensions.__file__))
    for line in trace.splitlines(True):
        if line.startswith('  File "'):
            path = line[len('  File "'):]
            if path.startswith(corepath):
                state = 'core'
            else:
                state = 'foreign'
        result += ui.label(line, 'traceback.%s' % state)
    return result

def _writeerr(orig, self, *args, **opts):
    text = ''.join(args)
    if text and text.startswith('Traceback'):
        text = _colorizetraceback(self, text)
    return orig(self, text, **opts)

def uisetup(ui):
    class morecolorsui(ui.__class__):
        def traceback(self, exc=None, force=False):
            if exc is None:
                exc = sys.exc_info()
            # wrap ui.write_err temporarily so we can capture the traceback and
            # add colors to it.
            cls = self.__class__
            extensions.wrapfunction(cls, 'write_err', _writeerr)
            try:
                return super(morecolorsui, self).traceback(exc, force)
            finally:
                extensions.unwrapfunction(cls, 'write_err', _writeerr)
    ui.__class__ = morecolorsui
