# logexceptions.py - Write files containing info about Mercurial exceptions
#
# Copyright 2017 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import inspect
import os
import sys
import traceback
import uuid

from mercurial import (
    dispatch,
    extensions,
)

def handleexception(orig, ui):
    res = orig(ui)

    if not ui.environ.get(b'HGEXCEPTIONSDIR'):
        return res

    dest = os.path.join(ui.environ[b'HGEXCEPTIONSDIR'],
                        str(uuid.uuid4()).encode('ascii'))

    exc_type, exc_value, exc_tb = sys.exc_info()

    stack = []
    tb = exc_tb
    while tb:
        stack.append(tb)
        tb = tb.tb_next
    stack.reverse()

    hgframe = 'unknown'
    hgline = 'unknown'

    # Find the first Mercurial frame in the stack.
    for tb in stack:
        mod = inspect.getmodule(tb)
        if not mod.__name__.startswith(('hg', 'mercurial')):
            continue

        frame = tb.tb_frame

        try:
            with open(inspect.getsourcefile(tb), 'r') as fh:
                hgline = fh.readlines()[frame.f_lineno - 1].strip()
        except (IndexError, OSError):
            pass

        hgframe = '%s:%d' % (frame.f_code.co_filename, frame.f_lineno)
        break

    primary = traceback.extract_tb(exc_tb)[-1]
    primaryframe = '%s:%d' % (primary.filename, primary.lineno)

    with open(dest, 'wb') as fh:
        parts = [
            str(exc_value),
            primaryframe,
            hgframe,
            hgline,
        ]
        fh.write(b'\0'.join(p.encode('utf-8', 'replace') for p in parts))

def extsetup(ui):
    extensions.wrapfunction(dispatch, 'handlecommandexception',
                            handleexception)
