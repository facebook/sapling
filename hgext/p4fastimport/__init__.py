# (c) 2017-present Facebook Inc.
"""p4fastimport - A fast importer from Perforce to Mercurial

Config example:

    [p4fastimport]
    # whether use worker or not
    useworker = false
    # trace copies?
    copytrace = false
    # Instead of uploading to LFS, store lfs metadata in this sqlite output
    # file. Some other process will upload from there to the LFS server later.
    lfsmetadata = PATH
    # path to sqlite output file for metadata
    metadata = PATH
    # certain commits by certain users should be igored so that
    # p4fastimporter imports the actual commits we want
    ignore-user = None
    # heuristic time difference between a ignored user commit
    # and a p4fastimporter import
    ignore-time-delta = None

    # The P4 database can become corrupted when it tracks symlinks to
    # directories. Keep this corruption out of the Mercurial repo.
    checksymlinks = True

"""
from __future__ import absolute_import

import collections
import itertools
import json
import sqlite3

from . import (
    importer,
    p4,
    seqimporter,
    syncimporter,
)
from .util import decodefileflags, getcl, lastcl, runworker

from mercurial.i18n import _
from mercurial.node import short, hex
from mercurial import (
    error,
    extensions,
    progress,
    registrar,
    revlog,
    scmutil,
    verify,
)

def extsetup():
    # Writing multiple changelog entries in one transaction can lead to revlog
    # caching issues when the inlined .i data is separated into a .d file. We
    # workaround by not allowing inlined revlogs at all.
    revlog.REVLOG_DEFAULT_VERSION = revlog.REVLOG_DEFAULT_FORMAT

def reposetup(ui, repo):
    def nothing(orig, *args, **kwargs):
        pass
    def yoloverify(orig, *args, **kwargs):
        # We have to set it directly as repo is reading the config lfs.bypass
        # during their repo setup.
        repo.svfs.options['lfsbypass'] = True
        return orig(*args, **kwargs)

    extensions.wrapfunction(verify.verifier, 'verify', yoloverify)

    if ui.config('p4fastimport', 'lfsmetadata', None) is not None:
        try:
            lfs = extensions.find('lfs')
        except KeyError:
            pass
        else:
            extensions.wrapfunction(lfs.blobstore.local, 'write', nothing)
            extensions.wrapfunction(lfs.blobstore.local, 'read', nothing)

def writebookmark(tr, repo, revisions, name):
    if len(revisions) > 0:
        __, hexnode = revisions[-1]
        repo._bookmarks.applychanges(repo, tr, [(name, repo[hexnode].node())])

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

def create(tr, ui, repo, importset, p1ctx, filelogs):
    for filelog in filelogs:
        # If the Perforce is case insensitive a filelog can map to
        # multiple filenames. For exmaple A.txt and a.txt would show up in the
        # same filelog. It would be more appropriate to update the filelist
        # after receiving the initial filelist but this would not be parallel.
        fi = importer.FileImporter(ui, repo, importset, filelog, p1ctx)
        fileflags, largefiles, baserevatcl, oldtiprev, newtiprev = fi.create(tr)
        yield 1, json.dumps({
            'newtiprev': newtiprev,
            'oldtiprev': oldtiprev,
            'fileflags': fileflags,
            'baserevatcl': baserevatcl,
            'largefiles': largefiles,
            'depotname': filelog.depotfile,
            'localname': fi.relpath,
        })

def enforce_p4_client_exists(client):
    # A client defines checkout behavior for a user. It contains a list of
    # views. A view defines a set of files and directories to check out from a
    # Perforce server and their mappins to local disk, e.g.:
    #   //depot/foo/... //client/x/...
    #    would map the files that are stored on the
    #   server under foo/* locally under x/*.
    if not p4.exists_client(client):
        raise error.Abort(_('p4 client %s does not exist.') % client)

def getchangelists(ui, client, startcl, limit=None):
    '''
        Returns a sorted list of changelists affecting client,
        starting at startcl.
        If a limit N is provided, return only the first N changelists.
    '''
    ui.note(_('loading changelist numbers.\n'))
    ignore_user = ui.config('p4fastimport', 'ignore-user')
    ignore_time_delta = ui.config('p4fastimport', 'ignore-time-delta')
    if ignore_user is None or ignore_time_delta is None:
        changelists = sorted(p4.parse_changes(client, startcl=startcl))
    else:
        changelists = list(itertools.takewhile(
            lambda cl: not (cl._user == ignore_user
                        and cl._commit_time_diff < ignore_time_delta),
            sorted(p4.parse_changes(client, startcl=startcl))))
    ui.note(_('%d changelists to import.\n') % len(changelists))

    if limit:
        limit = int(limit)
        if limit < len(changelists):
            ui.debug('importing %d only because of --limit.\n' % limit)
            changelists = changelists[:limit]
    return changelists

def getfilelist(ui, p4filelist):
    filelist = set()
    for fileinfo in p4filelist:
        if fileinfo['action'] in p4.ACTION_ARCHIVE:
            pass
        elif fileinfo['action'] in p4.SUPPORTED_ACTIONS:
            filelist.add(fileinfo['depotFile'])
        else:
            ui.warn(_('unknown action %s: %s\n') % (fileinfo['action'],
                                                    fileinfo['depotFile']))
    return filelist

def sanitizeopts(repo, opts):
    if opts.get('base') and not opts.get('bookmark'):
        raise error.Abort(_('must set --bookmark when using --base'))
    if opts.get('bookmark'):
        scmutil.checknewlabel(repo, opts['bookmark'], 'bookmark')
    limit = opts.get('limit')
    if limit:
        try:
            limit = int(limit)
        except ValueError:
            raise error.Abort(_('--limit should be an integer, got %s') % limit)
        if limit <= 0:
            raise error.Abort(_('--limit should be > 0, got %d') % limit)

def startfrom(ui, repo, opts):
    base, dest = 'null', 'tip'
    if opts.get('bookmark'):
        dest = opts.get('bookmark')
    if opts.get('base'):
        base = opts['base']
        if opts.get('bookmark') not in repo:
            dest = base

    basectx = scmutil.revsingle(repo, base)
    destctx = scmutil.revsingle(repo, dest)
    ctx = list(repo.set("""
        last(
          %n::%n and (
             extra(p4changelist) or
             extra(p4fullimportbasechangelist)))""",
             basectx.node(), destctx.node()))
    if ctx:
        ctx = ctx[0]
        startcl = lastcl(ctx)
        ui.note(_('incremental import from changelist: %d, node: %s\n') %
                (startcl, short(ctx.node())))
        if ctx.node() == basectx.node():
            ui.note(_('creating branchpoint, base %s\n') %
                    short(basectx.node()))
            return ctx, startcl, True
        return ctx, startcl, False
    raise error.Abort(_('no valid p4 changelist number.'))

def updatemetadata(ui, revisions, largefiles):
    lfsmetadata = ui.config('p4fastimport', 'lfsmetadata', None)
    if lfsmetadata is not None:
        if len(largefiles) > 0:
            ui.note(_('writing lfs metadata to sqlite\n'))
        writelfsmetadata(largefiles, revisions, lfsmetadata)

    metadata = ui.config('p4fastimport', 'metadata', None)
    if metadata is not None:
        if len(revisions) > 0:
            ui.note(_('writing metadata to sqlite\n'))
        writerevmetadata(revisions, metadata)

cmdtable = {}
command = registrar.command(cmdtable)

@command(
    'p4fastimport',
    [('P', 'path', '.', _('path to the local depot store'), _('PATH')),
     ('B', 'bookmark', '', _('bookmark to set'), _('NAME')),
     ('', 'base', '', _('base changeset (must exist in the repository)')),
     ('', 'limit', '',
         _('number of changelists to import at a time'), _('N'))],
    _('[-P PATH] [-B NAME] [--limit N] [CLIENT]'),
    inferrepo=True)
def p4fastimport(ui, repo, client, **opts):
    if 'fncache' in repo.requirements:
        raise error.Abort(_('fncache must be disabled'))

    sanitizeopts(repo, opts)

    if len(repo) > 0:
        p1ctx, startcl, isbranchpoint = startfrom(ui, repo, opts)
    else:
        p1ctx, startcl, isbranchpoint = repo['tip'], None, False

    # 0. Fail if the specified client does not exist
    enforce_p4_client_exists(client)
    # 1. Return all the changelists touching files in a given client view.
    changelists = getchangelists(ui, client, startcl, limit=opts.get('limit'))
    if len(changelists) == 0:
        return

    basepath = opts.get('path')
    startcl, endcl = changelists[0].cl, changelists[-1].cl

    # 2. Get a list of files that we will have to import from the depot with
    # it's full path in the depot.
    ui.note(_('loading list of files.\n'))
    filelist = getfilelist(ui, p4.parse_filelist(client, startcl, endcl))
    ui.note(_('%d files to import.\n') % len(filelist))

    importset = importer.ImportSet(repo, client, changelists,
            filelist, basepath, isbranchpoint=isbranchpoint)
    p4filelogs = []
    with progress.bar(ui, _('reading filelog'), _('filelogs'),
                            len(filelist)) as prog:
        for i, f in enumerate(importset.filelogs()):
            ui.debug('reading filelog %s\n' % f.depotfile)
            prog.value = i
            p4filelogs.append(f)

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
        for a, b in importset.caseconflicts:
            ui.warn(_('case conflict: %s and %s\n') % (a, b))
        # 3. Import files.
        count = 0
        fileinfo = {}
        largefiles = []
        ftr = repo.transaction('importer')
        try:
            with progress.bar(ui, _('importing filelogs'), 'file',
                              len(p4filelogs)) as prog:
                for filelogs in map(sorted, runlist.values()):
                    wargs = (ftr, ui, repo, importset, p1ctx)
                    for i, serialized in runworker(ui, create, wargs, filelogs):
                        data = json.loads(serialized)
                        prog.value = (count, data['depotname'])
                        # Json converts to UTF8 and int keys to strings, so we
                        # have to convert back.
                        # TODO: Find a better way to handle this.
                        fileinfo[data['depotname']] = {
                            'localname': data['localname'].encode('utf-8'),
                            'flags': decodefileflags(data['fileflags']),
                            'baserevatcl': data['baserevatcl'],
                        }
                        largefiles.extend(data['largefiles'])
                        count += i
                ftr.close()

            tr = repo.transaction('import')
            try:
                # 4. Generate manifest and changelog based on the filelogs
                # we imported
                clog = importer.ChangeManifestImporter(ui, repo, importset,
                        p1ctx=p1ctx)
                revisions = []
                for cl, hgnode in clog.creategen(tr, fileinfo):
                    revisions.append((cl, hex(hgnode)))

                if opts.get('bookmark'):
                    ui.note(_('writing bookmark\n'))
                    writebookmark(tr, repo, revisions, opts['bookmark'])

                updatemetadata(ui, revisions, largefiles)

                tr.close()
                ui.note(_('%d revision(s), %d file(s) imported.\n') % (
                    len(changelists), count))
            finally:
                tr.release()
        finally:
            ftr.release()

@command(
    'p4seqimport',
    [('P', 'path', '.', _('path to the local depot store'), _('PATH')),
     ('B', 'bookmark', '', _('bookmark to set'), _('NAME')),
     ('', 'base', '', _('base changeset (must exist in the repository)')),
     ('', 'limit', '', _('max number of changelists to import'), _('N'))],
    _('[-P PATH] [-B NAME] client'),
)
def p4seqimport(ui, repo, client, **opts):
    '''Sequentially import changelists'''
    if 'fncache' in repo.requirements:
        raise error.Abort(_('fncache must be disabled'))
    enforce_p4_client_exists(client)
    sanitizeopts(repo, opts)

    startcl = None
    ctx = repo['tip']
    if len(repo) > 0:
        ctx, startcl = startfrom(ui, repo, opts)[:2]

    changelists = getchangelists(ui, client, startcl, limit=opts.get('limit'))
    if len(changelists) == 0:
        ui.note(_('no changes to import, exiting.\n'))
        return

    climporter = seqimporter.ChangelistImporter(
        ui,
        repo,
        ctx,
        client,
        opts.get('path'),
    )
    with repo.wlock(), repo.lock(), repo.transaction('seqimport') as tr:
        node = None
        for p4cl in changelists:
            node, largefiles = climporter.importcl(p4cl)
            updatemetadata(ui, [(p4cl.cl, hex(node))], largefiles)
        if node is not None and opts.get('bookmark'):
            writebookmark(tr, repo, [(None, hex(node))], opts['bookmark'])

@command(
        'p4syncimport',
        [('P', 'path', '.', _('path to the local depot store'), _('PATH')),
         ('B', 'bookmark', '', _('bookmark to set'), _('NAME'))],
        _('[-P PATH] [-B NAME] oldclient newclient'),
        )
def p4syncimport(ui, repo, oldclient, newclient, **opts):
    sanitizeopts(repo, opts)
    storepath = opts.get('path')

    if len(repo) == 0:
        raise error.Abort(_('p4 sync commit does not support empty repo yet.'))

    p1ctx, startcl, __ = startfrom(ui, repo, opts)

    # Fail if the specified client does not exist
    enforce_p4_client_exists(oldclient)
    enforce_p4_client_exists(newclient)

    # Get a list of files that we will have to import
    oldcl = p4.get_latest_cl(oldclient)
    latestcl = p4.get_latest_cl(newclient)
    lastimportedcl = getcl(p1ctx)
    if latestcl is None:
        raise error.Abort(_('cannot find latest p4 changelist number'))
    ui.debug('%r (current client) %r (requested client) '
             '%r (latest imported)\n' % (oldcl, latestcl, lastimportedcl))
    if oldcl != lastimportedcl:
        # Consider running p4fastimport from here
        raise error.Abort(_('repository must contain most recent changes'))

    ui.note(_('latest change list number %s\n') % latestcl)

    filesadd, filesdel = syncimporter.get_filelogs_to_sync(
        ui,
        oldclient,
        oldcl,
        newclient,
        latestcl,
    )

    if not filesadd and not filesdel:
        ui.warn(_('nothing to import.\n'))
        return

    # sync import
    simporter = syncimporter.SyncImporter(
        ui,
        repo,
        p1ctx,
        storepath,
        latestcl,
        filesadd,
        filesdel
    )

    with repo.wlock(), repo.lock(), repo.transaction('syncimport') as tr:
        node, largefiles = simporter.sync_commit()
        updatemetadata(ui, [(latestcl, hex(node))], largefiles)
        if node is not None and opts.get('bookmark'):
            writebookmark(tr, repo, [(None, hex(node))], opts['bookmark'])

@command('debugscanlfs',
         [('r', 'rev', '.', _('display LFS files in REV')),
          ('A', 'all', None, _('display LFS files all revisions'))])
def debugscanlfs(ui, repo, **opts):
    lfs = extensions.find('lfs')
    def display(repo, filename, flog, rev):
        filenode = flog.node(rev)
        rawtext = flog.revision(filenode, raw=True)
        ptr = lfs.pointer.deserialize(rawtext)
        linkrev = flog.linkrev(rev)
        cl = int(repo[linkrev].extra()['p4changelist'])
        return _('%d %s %s %d %s\n') % (
            flog.linkrev(rev), hex(filenode), ptr.oid(), cl, filename)

    if opts.get('all'):
        prefix, suffix = "data/", ".i"
        plen, slen = len(prefix), len(suffix)
        for fn, b, size in repo.store.datafiles():
            if size == 0 or fn[-slen:] != suffix or fn[:plen] != prefix:
                continue
            fn = fn[plen:-slen]
            flog = repo.file(fn)
            for rev in range(0, len(flog)):
                flags = flog.flags(rev)
                if bool(flags & revlog.REVIDX_EXTSTORED):
                    ui.write(display(repo, fn, flog, rev))
    else:
        revisions = repo.set(opts.get('rev', '.'))
        for ctx in revisions:
            for fn in ctx.manifest():
                fctx = ctx[fn]
                flog = fctx.filelog()
                flags = flog.flags(fctx.filerev())
                if bool(flags & revlog.REVIDX_EXTSTORED):
                    ui.write(display(repo, fn, flog, fctx.filerev()))
