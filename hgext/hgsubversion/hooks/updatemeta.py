# Mercurial hook to update/rebuild svn metadata if there are svn changes in
# the incoming changegroup.
#
# To install, add the following to your hgrc:
# [hooks]
# changegroup = python:hgext.hgsubversion.hooks.updatemeta.hook

# no-check-code -- see T24862348

from mercurial import node

import hgext.hgsubversion
import hgext.hgsubversion.util
import hgext.hgsubversion.svncommands

def hook(ui, repo, **kwargs):
    updatemeta = False
    startrev = repo[node.bin(kwargs["node"])].rev()
    # Check each rev until we find one that contains svn metadata
    for rev in xrange(startrev, len(repo)):
        svnrev = hgext.hgsubversion.util.getsvnrev(repo[rev])
        if svnrev and svnrev.startswith("svn:"):
            updatemeta = True
            break

    if updatemeta:
        try:
            hgext.hgsubversion.svncommands.updatemeta(ui, repo, args=[])
            ui.status("Updated svn metadata\n")
        except Exception, e:
            ui.warn("Failed to update svn metadata: %s" % str(e))

    return False
