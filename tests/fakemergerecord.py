# Extension to write out fake unsupported records into the merge state
#
#

from __future__ import absolute_import

from mercurial import (
    merge,
    registrar,
)

cmdtable = {}
command = registrar.command(cmdtable)

@command('fakemergerecord',
         [('X', 'mandatory', None, 'add a fake mandatory record'),
          ('x', 'advisory', None, 'add a fake advisory record')], '')
def fakemergerecord(ui, repo, *pats, **opts):
    with repo.wlock():
        ms = merge.mergestate.read(repo)
        records = ms._makerecords()
        if opts.get('mandatory'):
            records.append(('X', 'mandatory record'))
        if opts.get('advisory'):
            records.append(('x', 'advisory record'))
        ms._writerecords(records)
