# Dummy extension to define a namespace containing revision names

from __future__ import absolute_import

from mercurial import (
    namespaces,
)

def reposetup(ui, repo):
    names = {'r%d' % rev: repo[rev].node() for rev in repo}
    namemap = lambda r, name: names.get(name)
    nodemap = lambda r, node: ['r%d' % repo[node].rev()]

    ns = namespaces.namespace('revnames', templatename='revname',
                              logname='revname',
                              listnames=lambda r: names.keys(),
                              namemap=namemap, nodemap=nodemap)
    repo.names.addnamespace(ns)
