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
    p4,
    importer,
    filetransaction as ftrmod
)

from .util import runworker, lastcl, decodefileflags

from mercurial.i18n import _
from mercurial.node import short, hex
from mercurial import (
    error,
    extensions,
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

    if opts.get('base') and not opts.get('bookmark'):
        raise error.Abort(_('must set --bookmark when using --base'))

    if opts.get('bookmark'):
        scmutil.checknewlabel(repo, opts['bookmark'], 'bookmark')

    if len(repo) > 0:
        p1ctx, startcl, isbranchpoint = startfrom(ui, repo, opts)
    else:
        p1ctx, startcl, isbranchpoint = repo['tip'], None, False

    # A client defines checkout behavior for a user. It contains a list of
    # views.A view defines a set of files and directories to check out from a
    # Perforce server and their mappins to local disk, e.g.:
    #   //depot/foo/... //client/x/...
    #    would map the files that are stored on the
    #   server under foo/* locally under x/*.

    # 0. Fail if the specified client does not exist
    if not p4.exists_client(client):
        raise error.Abort(_('p4 client %s does not exist.') % client)

    # 1. Return all the changelists touching files in a given client view.
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

    limit = len(changelists)
    if opts.get('limit'):
        limit = int(opts.get('limit'))
        changelists = changelists[0:limit]

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
    for i, f in enumerate(importset.filelogs()):
        ui.debug('reading filelog %s\n' % f.depotfile)
        ui.progress(_('reading filelog'), i, unit=_('filelogs'),
                total=len(filelist))
        p4filelogs.append(f)
    ui.progress(_('reading filelog'), None)

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
        ftr = ftrmod.filetransaction(ui.warn, repo.svfs)
        try:
            for filelogs in map(sorted, runlist.values()):
                wargs = (ftr, ui, repo, importset)
                for i, serialized in runworker(ui, create, wargs, filelogs):
                    data = json.loads(serialized)
                    ui.progress(_('importing filelogs'), count,
                            item=data['depotname'], unit='file',
                            total=len(p4filelogs))
                    # Json converts to UTF8 and int keys to strings, so we
                    # have to convert back.
                    # TODO: Find a better way to handle this.
                    fileinfo[data['depotname']] = {
                        'localname': data['localname'].encode('utf-8'),
                        'flags': decodefileflags(data['fileflags']),
                        'baserev': data['oldtiprev'],
                    }
                    largefiles.extend(data['largefiles'])
                    count += i
                ui.progress(_('importing filelogs'), None)
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
        finally:
            ftr.release()

@command(
        'p4syncimport',
        [('P', 'path', '.', _('path to the local depot store'), _('PATH')),
         ('B', 'bookmark', '', _('bookmark to set'), _('NAME'))],
        _('[-P PATH] client [-B NAME] bookmarkname'),
        )
def p4syncimport(ui, repo, client, **opts):
    if opts.get('bookmark'):
        scmutil.checknewlabel(repo, opts['bookmark'], 'bookmark')

    if len(repo) == 0:
        raise error.Abort(_('p4 sync commit does not support empty repo yet.'))

    p1ctx, startcl, __ = startfrom(ui, repo, opts)

    # Fail if the specified client does not exist
    if not p4.exists_client(client):
        raise error.Abort(_('p4 client %s does not exist.') % client)

    # Get a list of files that we will have to import
    latestcl = p4.get_latest_cl(client)
    if latestcl is None:
        raise error.Abort(_('Cannot find latest p4 changelist number.'))

    ui.note(_('Latest change list number %s\n') % latestcl)
    p4filelogs = p4.get_filelogs_at_cl(client, latestcl)
    p4filelogs = sorted(p4filelogs)
    newp4filelogs, reusep4filelogs = importer.get_filelogs_to_sync(
            client, repo, p1ctx, startcl - 1, p4filelogs)

    # sync import.
    with repo.wlock(), repo.lock():
        ui.note(_('running a sync import.\n'))
        count = 0
        fileinfo = {}
        largefileslist = []
        tr = repo.transaction('syncimport')
        try:
            for p4fl, localname in newp4filelogs:
                bfi = importer.SyncFileImporter(
                        ui, repo, client, latestcl, p4fl, localfile=localname)
                # Create hg filelog
                fileflags, largefiles, oldtiprev, newtiprev = bfi.create(tr)
                fileinfo[p4fl.depotfile] = {
                    'flags': fileflags,
                    'localname': bfi.relpath,
                    'baserev': oldtiprev,
                }
                largefileslist.extend(largefiles)
                count += 1
            # Generate manifest and changelog
            clog = importer.SyncChangeManifestImporter(
                     ui, repo, client, latestcl, p1ctx=p1ctx)
            revisions = []
            for cl, hgnode in clog.creategen(tr, fileinfo, reusep4filelogs):
                revisions.append((cl, hex(hgnode)))

            if opts.get('bookmark'):
                ui.note(_('writing bookmark\n'))
                writebookmark(tr, repo, revisions, opts['bookmark'])

            if ui.config('p4fastimport', 'lfsmetadata', None) is not None:
                ui.note(_('writing lfs metadata to sqlite\n'))
                writelfsmetadata(largefileslist, revisions,
                    ui.config('p4fastimport', 'lfsmetadata', None))

            tr.close()
            ui.note(_('1 revision, %d file(s) imported.\n') % count)
        finally:
            tr.release()

@command('debugscanlfs',
         [('C', 'client', '', _('Perforce client to reverse lookup')),
          ('r', 'rev', '.', _('display LFS files in REV')),
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

    def batchfnmap(repo, client, infos):
        for filename, flog, rev in infos:
            whereinfo = p4.parse_where(client, filename)
            yield 1, display(repo, whereinfo['depotFile'], flog, rev)

    client = opts.get('client', None)
    todisplay = []
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
                    if client:
                        todisplay.append((fn, flog, rev))
                    else:
                        ui.write(display(repo, fn, flog, rev))
    else:
        revisions = repo.set(opts.get('rev', '.'))
        for ctx in revisions:
            for fn in ctx.manifest():
                fctx = ctx[fn]
                flog = fctx.filelog()
                flags = flog.flags(fctx.filerev())
                if bool(flags & revlog.REVIDX_EXTSTORED):
                    if client:
                        todisplay.append((fn, flog, fctx.filerev()))
                    else:
                        ui.write(display(repo, fn, flog, fctx.filerev()))
    if todisplay:
        args = (repo, client)
        for i, s in runworker(ui, batchfnmap, args, todisplay):
            ui.write(s)
