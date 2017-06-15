# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import json

from .backupcommands import cmdtable as backupcmdtable

from mercurial import (
    # cmdutil,
    encoding,
    error,
    hg,
    patch,
    registrar,
    scmutil,
    util,
)

from .common import downloadbundle
from mercurial.node import bin
from mercurial.i18n import _

cmdtable = backupcmdtable
command = registrar.command(cmdtable)

@command('debugfillinfinitepushmetadata',
         [('', 'node', '', 'node to fill metadata for')])
def debugfillinfinitepushmetadata(ui, repo, node):
    '''Special command that fills infinitepush metadata for a node
    '''

    if not node:
        raise error.Abort(_('node is not specified'))

    index = repo.bundlestore.index
    if not bool(index.getbundle(node)):
        raise error.Abort(_('node %s is not found') % node)

    newbundlefile = downloadbundle(repo, bin(node))
    bundlepath = "bundle:%s+%s" % (repo.root, newbundlefile)
    bundlerepo = hg.repository(repo.ui, bundlepath)
    repo = bundlerepo

    p1 = repo[node].p1().node()
    diffopts = patch.diffallopts(ui, {})
    match = scmutil.matchall(repo)
    chunks = patch.diff(repo, p1, node, match, None, diffopts, relroot='')
    difflines = util.iterlines(chunks)

    diffstat = patch.diffstatdata(difflines)
    changed_files = {}
    for filename, adds, removes, isbinary in diffstat:
        # use special encoding that allows non-utf8 filenames
        filename = encoding.jsonescape(filename, paranoid=True)
        changed_files[filename] = {
            'adds': adds, 'removes': removes, 'isbinary': isbinary,
        }
    output = {}
    output['changed_files'] = changed_files
    index.saveoptionaljsonmetadata(node, json.dumps(output, sort_keys=True))
