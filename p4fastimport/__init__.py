# (c) 2017-present Facebook Inc.
"""p4fastimport - A fast importer from Perforce to Mercurial

Config example:

    [p4fastimport]
    # whether use worker or not
    useworker = false
    # trace copies?
    copytrace = false
    # if LFS is enabled, write only the metadata to disk, do not write the
    # blob itself to the local cache.
    lfspointeronly = false
    # path to sqlite output file for lfs metadata
    lfsmetadata = PATH
    # path to sqlite output file for metadata
    metadata = PATH

"""
from __future__ import absolute_import

import collections
import json
import sqlite3

from . import (
    p4,
    importer,
    util,
)

from mercurial.i18n import _
from mercurial.node import short, hex
from mercurial import (
    cmdutil,
    error,
    extensions,
    scmutil,
    verify,
    worker,
)

def reposetup(ui, repo):
    def nothing(orig, *args, **kwargs):
        pass
    def yoloverify(orig, *args, **kwargs):
        # We have to set it directly as repo is reading the config lfs.bypass
        # during their repo setup.
        repo.svfs.options['lfsbypass'] = True
        return orig(*args, **kwargs)
    def handlelfs(loaded):
        if loaded:
            lfs = extensions.find('lfs')
            extensions.wrapfunction(lfs.blobstore.local, 'write', nothing)
            extensions.wrapfunction(lfs.blobstore.local, 'read', nothing)

    extensions.wrapfunction(verify.verifier, 'verify', yoloverify)
    extensions.afterloaded('lfs', handlelfs)

def writebookmark(tr, repo, revisions, name):
    if len(revisions) > 0:
        marks = repo._bookmarks
        __, hexnode = revisions[-1]
        marks[name] = repo[hexnode].node()
        marks.recordchange(tr)

def writerevmetadata(revisions, outfile):
    """Write the LFS mappings from OID to a depotpath and it's CLnum into
    sqlite. This way the LFS server can import the correct file from Perforce
    and mapping it to the correct OID.
    """
    with sqlite3.connect(outfile, isolation_level=None) as conn:
        cur = conn.cursor()
        cur.execute("BEGIN TRANSACTION")
        cur.execute("""
        CREATE TABLE IF NOT EXISTS revision_mapping (
            "id" INTEGER PRIMARY KEY AUTOINCREMENT,
            "cl" INTEGER NOT NULL,
            "node" BLOB
        )""")
        cur.executemany(
            "INSERT INTO revision_mapping(cl, node) VALUES (?,?)",
            revisions)
        cur.execute("COMMIT")

def writelfsmetadata(largefiles, revisions, outfile):
    """Write the LFS mappings from OID to a depotpath and it's CLnum into
    sqlite. This way the LFS server can import the correct file from Perforce
    and mapping it to the correct OID.
    """
    with sqlite3.connect(outfile, isolation_level=None) as conn:
        cur = conn.cursor()
        cur.execute("BEGIN TRANSACTION")
        cur.execute("""
        CREATE TABLE IF NOT EXISTS p4_lfs_map(
            "id" INTEGER PRIMARY KEY AUTOINCREMENT,
            "cl" INTEGER NOT NULL,
            "node" BLOB,
            "oid" TEXT,
            "path" BLOB
        )""")
        inserts = []
        revdict = dict(revisions)
        for cl, path, oid in largefiles:
            inserts.append((cl, path, oid, revdict[cl]))

        cur.executemany(
            "INSERT INTO p4_lfs_map(cl, path, oid, node) VALUES (?,?,?,?)",
            inserts)
        cur.execute("COMMIT")

def create(tr, ui, repo, importset, filelogs):
    for filelog in filelogs:
        # If the Perforce is case insensitive a filelog can map to
        # multiple filenames. For exmaple A.txt and a.txt would show up in the
        # same filelog. It would be more appropriate to update the filelist
        # after receiving the initial filelist but this would not be parallel.
        fi = importer.FileImporter(ui, repo, importset, filelog)
        fileflags, largefiles, oldtiprev, newtiprev = fi.create(tr)
        yield 1, json.dumps({
            'newtiprev': newtiprev,
            'oldtiprev': oldtiprev,
            'fileflags': fileflags,
            'largefiles': largefiles,
            'depotname': filelog.depotfile,
            'localname': fi.relpath,
        })

cmdtable = {}
command = cmdutil.command(cmdtable)

def runworker(ui, fn, wargs, items):
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
    return worker.worker(ui, weight, fn, wargs, items)

@command(
    'p4fastimport',
    [('P', 'path', '.', _('path to the local depot store'), _('PATH')),
     ('B', 'bookmark', '', _('bookmark to set'), _('NAME'))],
    _('[-P PATH] [-B NAME] [CLIENT]'),
    inferrepo=True)
def p4fastimport(ui, repo, client, **opts):
    if 'fncache' in repo.requirements:
        raise error.Abort(_('fncache must be disabled'))

    basepath = opts.get('path')

    if opts.get('bookmark'):
        scmutil.checknewlabel(repo, opts['bookmark'], 'bookmark')

    startcl = None
    if len(repo) > 0 and startcl is None:
        latestctx = list(repo.set("last(extra(p4changelist))"))
        if latestctx:
            startcl = util.lastcl(latestctx[0])
            ui.note(_('incremental import from changelist: %d, node: %s\n') %
                    (startcl, short(latestctx[0].node())))

    # A client defines checkout behavior for a user. It contains a list of
    # views.A view defines a set of files and directories to check out from a
    # Perforce server and their mappins to local disk, e.g.:
    #   //depot/foo/... //client/x/...
    #    would map the files that are stored on the
    #   server under foo/* locally under x/*.
    # 1. Return all the changelists touching files in a given client view.
    ui.note(_('loading changelist numbers.\n'))
    changelists = list(p4.parse_changes(client, startcl=startcl))
    ui.note(_('%d changelists to import.\n') % len(changelists))

    # 2. Get a list of files that we will have to import from the depot with
    # it's full path in the depot.
    ui.note(_('loading list of files.\n'))
    filelist = set()
    for fileinfo in p4.parse_filelist(client, startcl=startcl):
        if fileinfo['action'] in p4.SUPPORTED_ACTIONS:
            filelist.add(fileinfo['depotFile'])
        else:
            ui.warn(_('unknown action %s: %s\n') % (fileinfo['action'],
                                                    fileinfo['depotFile']))
    ui.note(_('%d files to import.\n') % len(filelist))

    importset = importer.ImportSet(repo, client, changelists,
            filelist, basepath)
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
    else:
        runlist[0] = p4filelogs

    ui.note(_('importing repository.\n'))
    with repo.wlock(), repo.lock():
        tr = repo.transaction('import')
        try:
            for a, b in importset.caseconflicts:
                ui.warn(_('case conflict: %s and %s\n') % (a, b))

            # 3. Import files.
            count = 0
            fileinfo = {}
            largefiles = []
            for filelogs in map(sorted, runlist.values()):
                wargs = (tr, ui, repo, importset)
                for i, serialized in runworker(ui, create, wargs, filelogs):
                    data = json.loads(serialized)
                    ui.progress(_('importing'), count,
                            item=data['depotname'], unit='file',
                            total=len(p4filelogs))
                    # Json converts to UTF8 and int keys to strings, so we
                    # have to convert back.
                    # TODO: Find a better way to handle this.
                    fileinfo[data['depotname']] = {
                        'localname': data['localname'].encode('utf-8'),
                        'flags': util.decodefileflags(data['fileflags']),
                        'baserev': data['oldtiprev'],
                    }
                    largefiles.extend(data['largefiles'])
                    count += i
                ui.progress(_('importing'), None)

            # 4. Generate manifest and changelog based on the filelogs
            # we imported
            clog = importer.ChangeManifestImporter(ui, repo, importset)
            revisions = []
            for cl, hgnode in clog.creategen(tr, fileinfo):
                revisions.append((cl, hex(hgnode)))

            if opts.get('bookmark'):
                ui.note(_('writing bookmark\n'))
                writebookmark(tr, repo, revisions, opts['bookmark'])

            if ui.config('p4fastimport', 'lfsmetadata', None) is not None:
                ui.note(_('writing lfs metadata to sqlite\n'))
                writelfsmetadata(largefiles, revisions,
                     ui.config('p4fastimport', 'lfsmetadata', None))

            if ui.config('p4fastimport', 'metadata', None) is not None:
                ui.note(_('writing metadata to sqlite\n'))
                writerevmetadata(revisions,
                     ui.config('p4fastimport', 'metadata', None))

            tr.close()
            ui.note(_('%d revision(s), %d file(s) imported.\n') % (
                len(changelists), count))
        finally:
            tr.release()
