# Copyright 2016-present Facebook. All Rights Reserved.
#
# hgwebsupport: fastannotate support for hgweb
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import (
    extensions,
    patch,
)
from mercurial.hgweb import webutil

from fastannotate import context

def _annotate(orig, fctx, ui):
    diffopts = patch.difffeatureopts(ui, untrusted=True,
                                     section='annotate', whitespace=True)
    aopts = context.annotateopts(diffopts=diffopts)
    master = ui.config('fastannotate', 'mainbranch', 'default')
    with context.annotatecontext(fctx.repo(), fctx.path(), aopts) as ac:
        # fastannotate returns: [(nodeid, linenum, path)], [linecontent]
        annotated, contents = ac.annotate(fctx.rev(), master=master,
                                          showpath=True, showlines=True)

    # convert to what fctx.annotate returns: [((fctx, number), linecontent)]
    fctxmap = {} # {(nodeid, path): fctx}
    repo = fctx.repo()
    results = []
    for i, (hsh, linenum, path) in enumerate(annotated):
        if (hsh, path) not in fctxmap:
            fctxmap[(hsh, path)] = context.resolvefctx(repo, hsh, path)
        results.append(((fctxmap[(hsh, path)], linenum), contents[i]))

    return results

def replacehgwebannotate():
    extensions.wrapfunction(webutil, 'annotate', _annotate)
