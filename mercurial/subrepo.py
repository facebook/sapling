# subrepo.py - sub-repository handling for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

import config, util, errno

def state(ctx):
    p = config.config()
    def read(f, sections=None, remap=None):
        if f in ctx:
            try:
                p.parse(f, ctx[f].data(), sections, remap)
            except IOError, err:
                if err.errno != errno.ENOENT:
                    raise
    read('.hgsub')

    rev = {}
    if '.hgsubstate' in ctx:
        try:
            for l in ctx['.hgsubstate'].data().splitlines():
                revision, path = l.split()
                rev[path] = revision
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise

    state = {}
    for path, src in p[''].items():
        state[path] = (src, rev.get(path, ''))

    return state
