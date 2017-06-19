# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import json

from .backupcommands import cmdtable as backupcmdtable

from mercurial import (
    copies as copiesmod,
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

    states = 'modified added removed deleted unknown ignored clean'.split()
    status = repo.status(p1, node)
    status = zip(states, status)

    filestatus = {}
    for state, files in status:
        for f in files:
            filestatus[f] = state

    diffstat = patch.diffstatdata(difflines)
    changed_files = {}
    copies = copiesmod.pathcopies(repo[p1], repo[node])
    for filename, adds, removes, isbinary in diffstat:
        # use special encoding that allows non-utf8 filenames
        filename = encoding.jsonescape(filename, paranoid=True)
        changed_files[filename] = {
            'adds': adds, 'removes': removes, 'isbinary': isbinary,
            'status': filestatus.get(filename, 'unknown')
        }
        if filename in copies:
            changed_files[filename]['copies'] = copies[filename]
    output = {}
    output['changed_files'] = changed_files
    with index:
        index.saveoptionaljsonmetadata(node, json.dumps(output, sort_keys=True))
