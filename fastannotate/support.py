# Copyright 2016-present Facebook. All Rights Reserved.
#
# support: fastannotate support for hgweb, and filectx
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import (
    context as hgcontext,
    extensions,
    hgweb,
    patch,
    util,
)

from . import context

class _lazyfctx(object):
    """delegates to fctx but do not construct fctx when unnecessary"""

    def __init__(self, repo, node, path):
        self._node = node
        self._path = path
        self._repo = repo

    def node(self):
        return self._node

    def path(self):
        return self._path

    @util.propertycache
    def _fctx(self):
        return context.resolvefctx(self._repo, self._node, self._path)

    def __getattr__(self, name):
        return getattr(self._fctx, name)

def _convertoutputs(repo, annotated, contents):
    """convert fastannotate outputs to vanilla annotate format"""
    # fastannotate returns: [(nodeid, linenum, path)], [linecontent]
    # convert to what fctx.annotate returns: [((fctx, linenum), linecontent)]
    results = []
    fctxmap = {}
    for i, (hsh, linenum, path) in enumerate(annotated):
        if (hsh, path) not in fctxmap:
            fctxmap[(hsh, path)] = _lazyfctx(repo, hsh, path)
        # linenum: the user wants 1-based, we have 0-based.
        results.append(((fctxmap[(hsh, path)], linenum + 1), contents[i]))
    return results

def _getmaster(fctx):
    """(fctx) -> str"""
    return fctx._repo.ui.config('fastannotate', 'mainbranch') or 'default'

def _doannotate(fctx, follow=True, diffopts=None):
    """like the vanilla fctx.annotate, but do it via fastannotate, and make
    the output format compatible with the vanilla fctx.annotate.
    may raise Exception, and always return line numbers.
    """
    master = _getmaster(fctx)
    annotated = contents = None

    with context.fctxannotatecontext(fctx, follow, diffopts) as ac:
        try:
            annotated, contents = ac.annotate(fctx.rev(), master=master,
                                              showpath=True, showlines=True)
        except Exception:
            ac.rebuild() # try rebuild once
            fctx._repo.ui.debug('fastannotate: %s: rebuilding broken cache\n'
                                % fctx._path)
            try:
                annotated, contents = ac.annotate(fctx.rev(), master=master,
                                                  showpath=True, showlines=True)
            except Exception:
                raise

    assert annotated and contents
    return _convertoutputs(fctx._repo, annotated, contents)

def _hgwebannotate(orig, fctx, ui):
    diffopts = patch.difffeatureopts(ui, untrusted=True,
                                     section='annotate', whitespace=True)
    return _doannotate(fctx, diffopts=diffopts)

def _fctxannotate(orig, self, follow=False, linenumber=False, diffopts=None):
    try:
        return _doannotate(self, follow, diffopts)
    except Exception as ex:
        self._repo.ui.debug('fastannotate: falling back to the vanilla '
                            'annotate: %r' % ex)
        return orig(self, follow, linenumber, diffopts)

def replacehgwebannotate():
    extensions.wrapfunction(hgweb.webutil, 'annotate', _hgwebannotate)

def replacefctxannotate():
    extensions.wrapfunction(hgcontext.basefilectx, 'annotate', _fctxannotate)
