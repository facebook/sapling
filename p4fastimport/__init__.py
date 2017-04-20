# (c) 2017-present Facebook Inc.
"""p4fastimport - A fast importer from Perforce to Mercurial

Config example:

    [p4fastimport]
    # whether use worker or not
    useworker = false
    # trace copies?
    copytrace = false

"""
from __future__ import absolute_import

import collections
import json

from . import (
    p4,
    importer,
    util,
)

from mercurial.i18n import _
from mercurial import (
    cmdutil,
    error,
    worker,
)

def create(tr, ui, repo, importset, filelogs):
    for filelog in filelogs:
        # If the Perforce is case insensitive a filelog can map to
        # multiple filenames. For exmaple A.txt and a.txt would show up in the
        # same filelog. It would be more appropriate to update the filelist
        # after receiving the initial filelist but this would not be parallel.
        fi = importer.FileImporter(ui, repo, importset, filelog)
        fileflags = fi.create(tr)
        yield 1, json.dumps({
            'fileflags': fileflags,
            'depotname': filelog.depotfile,
        })

# -> Dict[Int, List[str]]
#def create_runlist(ui, repo, filelist, path):
#    def follow(fi, depmap):
#        # XXX: Careful about stackoverflow
#        if fi.dependency[0] is not None:
#            # XXX: Don't visit the same files twice
#            flog = importer.FileImporter(ui, repo, path, fi.dependency[1])
#            add, depmap = follow(flog, depmap)
#            depmap[fi._depotfname] += add
#        return depmap[fi._depotfname] + 1, depmap
#
#    depmap = collections.defaultdict(lambda: 0)
#    for filename in filelist:
#        fi = importer.FileImporter(ui, repo, path, filename)
#        __, depmap = follow(fi, depmap)
#    runlist = collections.defaultdict(list)
#    for k, v in depmap.iteritems():
#        runlist[v].append(k)
#    return runlist

cmdtable = {}
command = cmdutil.command(cmdtable)

@command(
    'p4fastimport',
    [('s', 'start', None, _('start of the CL range to import'), _('REV')),
     ('e', 'end', None, _('end of the CL range to import'), _('REV')),
     ('P', 'path', '.', _('path to the local depot store'), _('PATH'))],
    _('hg p4fastimport [-s start] [-e end] [-P PATH] [CLIENT]'),
    inferrepo=True)
def p4fastimport(ui, repo, client, **opts):
    if 'fncache' in repo.requirements:
        raise error.Abort(_('fncache must be disabled'))

    basepath = opts.get('path')

    # A client defines checkout behavior for a user. It contains a list of
    # views.A view defines a set of files and directories to check out from a
    # Perforce server and their mappins to local disk, e.g.:
    #   //depot/foo/... //client/x/...
    #    would map the files that are stored on the
    #   server under foo/* locally under x/*.
    # 1. Return all the changelists touching files in a given client view.
    ui.note(_('loading changelist numbers.\n'))
    changelists = list(p4.parse_changes(client))
    ui.note(_('%d changelists to import.\n') % len(changelists))

    # 2. Get a list of files that we will have to import from the depot with
    # it's full path in the depot.
    ui.note(_('loading list of files.\n'))
    filelist = set()
    for fileinfo in p4.parse_filelist(client):
        if fileinfo['action'] in p4.SUPPORTED_ACTIONS:
            filelist.add(fileinfo['depotFile'])
        else:
            ui.warn(_('unknown action %s: %s\n') % (fileinfo['action'],
                                                    fileinfo['depotFile']))
    ui.note(_('%d files to import.\n') % len(filelist))

    importset = importer.ImportSet(changelists, filelist, storagepath=basepath)

    p4filelogs = []
    for i, f in enumerate(importset.filelogs()):
        ui.progress(_('loading filelog'), i, item=f, unit="filelog",
                total=len(filelist))
        p4filelogs.append(f)
    ui.progress(_('loading filelog'), None)

    # runlist is used to topologically order files which were branched (Perforce
    # uses per-file branching, not per-repo branching).  If we do copytracing a
    # file A' which was branched off A will be considered a copy of A. Therefore
    # we need to import A' before A. In this case A' will have a dependency
    # counter +1 of A's, and therefore being imported after A. If copy tracing
    # is disabled this is not needed and we can import files in arbitrary order.
    runlist = collections.OrderedDict()
    if ui.configbool('p4fastimport', 'copytrace', False):
        raise error.Abort(_('copytracing is broken'))
    #        ui.note(_('Tracing file copies.\n'))
    #        runlist = create_runlist(ui, repo, changelists, linkrevmap,
    #           filelist, basepath)
    #        copy_tracer = importer.CopyTracer(filelist)
    else:
        runlist[0] = p4filelogs

    ui.note(_('importing repository.\n'))
    wlock = repo.wlock()
    lock = repo.lock()
    tr = None
    try:
        tr = repo.transaction('import')
        for a, b in importset.caseconflicts:
            ui.warn(_('case conflict: %s and %s\n') % (a, b))

        # 3. Import files.
        count = 0
        fileflags = {}
        for filelogs in map(sorted, runlist.values()):
            wargs = (tr, ui, repo, importset)

            # 0.4 is the cost per argument. So if we have at least 100 files
            # on a 4 core machine than our linear cost outweights the
            # drawback of spwaning. We are overwritign this if we force a
            # worker to run with a ridiculous high number.
            weight = 0.0  # disable worker
            if ui.config('p4fastimport', 'useworker', None) == 'force':
                weight = 100000.0  # force worker
            elif ui.configbool('p4fastimport', 'useworker', False):
                weight = 0.04  # normal weight

            # Fix duplicated messages before
            # https://www.mercurial-scm.org/repo/hg-committed/rev/9d3d56aa1a9f
            ui.flush()
            prog = worker.worker(ui, weight, create, wargs, filelogs)
            for i, serialized in prog:
                data = json.loads(serialized)
                ui.progress(_('importing'), count, item=data['depotname'],
                            unit='file', total=len(p4filelogs))
                # Json converts to UTF8 and int keys to strings, so we have to
                # convert back. TODO: Find a better way to handle this.
                fileflags.update(util.decodefileflags(data['fileflags']))
                count += i
            ui.progress(_('importing'), None)

        # 4. Generate manifest and changelog based on the filelogs we imported
        clog = importer.ChangeManifestImporter(ui, repo, importset)
        clog.create(tr, fileflags)
        tr.close()
        ui.note(_('%d revision(s), %d file(s) imported.\n') % (
            len(changelists), count))
    finally:
        if tr:
            tr.release()
        lock.release()
        wlock.release()
