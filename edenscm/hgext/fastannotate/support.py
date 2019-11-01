# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# support: fastannotate support for hgweb, and filectx

from __future__ import absolute_import

from edenscm.mercurial import context as hgcontext, extensions, hgweb, patch, util

from . import context, revmap


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
    annotateline = getattr(hgcontext, "annotateline", None)
    for i, (hsh, linenum, path) in enumerate(annotated):
        if (hsh, path) not in fctxmap:
            fctxmap[(hsh, path)] = _lazyfctx(repo, hsh, path)
        # linenum: the user wants 1-based, we have 0-based.
        lineno = linenum + 1
        fctx = fctxmap[(hsh, path)]
        line = contents[i]
        if annotateline is None:
            results.append(((fctx, lineno), line))
        else:
            # 2e32c6a31cc7 introduced annotateline
            results.append((annotateline(fctx=fctx, lineno=lineno), line))
    return results


def _getmaster(fctx):
    """(fctx) -> str"""
    return fctx._repo.ui.config("fastannotate", "mainbranch") or "default"


def _doannotate(fctx, follow=True, diffopts=None):
    """like the vanilla fctx.annotate, but do it via fastannotate, and make
    the output format compatible with the vanilla fctx.annotate.
    may raise Exception, and always return line numbers.
    """
    master = _getmaster(fctx)
    annotated = contents = None

    with context.fctxannotatecontext(fctx, follow, diffopts) as ac:
        try:
            annotated, contents = ac.annotate(
                fctx.rev(), master=master, showpath=True, showlines=True
            )
        except Exception:
            ac.rebuild()  # try rebuild once
            fctx._repo.ui.debug(
                "fastannotate: %s: rebuilding broken cache\n" % fctx._path
            )
            try:
                annotated, contents = ac.annotate(
                    fctx.rev(), master=master, showpath=True, showlines=True
                )
            except Exception:
                raise

    assert annotated and contents
    return _convertoutputs(fctx._repo, annotated, contents)


def _hgwebannotate(orig, fctx, ui):
    diffopts = patch.difffeatureopts(
        ui, untrusted=True, section="annotate", whitespace=True
    )
    return _doannotate(fctx, diffopts=diffopts)


def _fctxannotate(
    orig, self, follow=False, linenumber=False, skiprevs=None, diffopts=None
):
    if skiprevs:
        # skiprevs is not supported yet
        return orig(self, follow, linenumber, skiprevs=skiprevs, diffopts=diffopts)
    try:
        return _doannotate(self, follow, diffopts)
    except Exception as ex:
        self._repo.ui.debug(
            "fastannotate: falling back to the vanilla " "annotate: %r\n" % ex
        )
        return orig(self, follow, linenumber, skiprevs=skiprevs, diffopts=diffopts)


def _remotefctxannotate(
    orig,
    self,
    follow=False,
    linenumber=None,
    skiprevs=None,
    diffopts=None,
    prefetchskip=None,
):
    # skipset: a set-like used to test if a fctx needs to be downloaded
    skipset = None
    with context.fctxannotatecontext(self, follow, diffopts) as ac:
        skipset = revmap.revmap(ac.revmappath)
    return orig(
        self,
        follow,
        linenumber,
        skiprevs=skiprevs,
        diffopts=diffopts,
        prefetchskip=skipset,
    )


def replacehgwebannotate():
    extensions.wrapfunction(hgweb.webutil, "annotate", _hgwebannotate)


def replacefctxannotate():
    extensions.wrapfunction(hgcontext.basefilectx, "annotate", _fctxannotate)


def replaceremotefctxannotate():
    try:
        r = extensions.find("remotefilelog")
    except KeyError:
        return
    else:
        extensions.wrapfunction(
            r.remotefilectx.remotefilectx, "annotate", _remotefctxannotate
        )
