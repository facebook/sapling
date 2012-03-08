# Copyright 2009-2010 Gregory P. Ward
# Copyright 2009-2010 Intelerad Medical Systems Incorporated
# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''High-level command function for lfconvert, plus the cmdtable.'''

import os
import shutil

from mercurial import util, match as match_, hg, node, context, error
from mercurial.i18n import _

import lfutil
import basestore

# -- Commands ----------------------------------------------------------

def lfconvert(ui, src, dest, *pats, **opts):
    '''convert a normal repository to a largefiles repository

    Convert repository SOURCE to a new repository DEST, identical to
    SOURCE except that certain files will be converted as largefiles:
    specifically, any file that matches any PATTERN *or* whose size is
    above the minimum size threshold is converted as a largefile. The
    size used to determine whether or not to track a file as a
    largefile is the size of the first version of the file. The
    minimum size can be specified either with --size or in
    configuration as ``largefiles.size``.

    After running this command you will need to make sure that
    largefiles is enabled anywhere you intend to push the new
    repository.

    Use --to-normal to convert largefiles back to normal files; after
    this, the DEST repository can be used without largefiles at all.'''

    if opts['to_normal']:
        tolfile = False
    else:
        tolfile = True
        size = lfutil.getminsize(ui, True, opts.get('size'), default=None)

    if not hg.islocal(src):
        raise util.Abort(_('%s is not a local Mercurial repo') % src)
    if not hg.islocal(dest):
        raise util.Abort(_('%s is not a local Mercurial repo') % dest)

    rsrc = hg.repository(ui, src)
    ui.status(_('initializing destination %s\n') % dest)
    rdst = hg.repository(ui, dest, create=True)

    success = False
    try:
        # Lock destination to prevent modification while it is converted to.
        # Don't need to lock src because we are just reading from its history
        # which can't change.
        dst_lock = rdst.lock()

        # Get a list of all changesets in the source.  The easy way to do this
        # is to simply walk the changelog, using changelog.nodesbewteen().
        # Take a look at mercurial/revlog.py:639 for more details.
        # Use a generator instead of a list to decrease memory usage
        ctxs = (rsrc[ctx] for ctx in rsrc.changelog.nodesbetween(None,
            rsrc.heads())[0])
        revmap = {node.nullid: node.nullid}
        if tolfile:
            lfiles = set()
            normalfiles = set()
            if not pats:
                pats = ui.configlist(lfutil.longname, 'patterns', default=[])
            if pats:
                matcher = match_.match(rsrc.root, '', list(pats))
            else:
                matcher = None

            lfiletohash = {}
            for ctx in ctxs:
                ui.progress(_('converting revisions'), ctx.rev(),
                    unit=_('revision'), total=rsrc['tip'].rev())
                _lfconvert_addchangeset(rsrc, rdst, ctx, revmap,
                    lfiles, normalfiles, matcher, size, lfiletohash)
            ui.progress(_('converting revisions'), None)

            if os.path.exists(rdst.wjoin(lfutil.shortname)):
                shutil.rmtree(rdst.wjoin(lfutil.shortname))

            for f in lfiletohash.keys():
                if os.path.isfile(rdst.wjoin(f)):
                    os.unlink(rdst.wjoin(f))
                try:
                    os.removedirs(os.path.dirname(rdst.wjoin(f)))
                except OSError:
                    pass

            # If there were any files converted to largefiles, add largefiles
            # to the destination repository's requirements.
            if lfiles:
                rdst.requirements.add('largefiles')
                rdst._writerequirements()
        else:
            for ctx in ctxs:
                ui.progress(_('converting revisions'), ctx.rev(),
                    unit=_('revision'), total=rsrc['tip'].rev())
                _addchangeset(ui, rsrc, rdst, ctx, revmap)

            ui.progress(_('converting revisions'), None)
        success = True
    finally:
        if not success:
            # we failed, remove the new directory
            shutil.rmtree(rdst.root)
        dst_lock.release()

def _addchangeset(ui, rsrc, rdst, ctx, revmap):
 # Convert src parents to dst parents
    parents = _convertparents(ctx, revmap)

    # Generate list of changed files
    files = _getchangedfiles(ctx, parents)

    def getfilectx(repo, memctx, f):
        if lfutil.standin(f) in files:
            # if the file isn't in the manifest then it was removed
            # or renamed, raise IOError to indicate this
            try:
                fctx = ctx.filectx(lfutil.standin(f))
            except error.LookupError:
                raise IOError()
            renamed = fctx.renamed()
            if renamed:
                renamed = lfutil.splitstandin(renamed[0])

            hash = fctx.data().strip()
            path = lfutil.findfile(rsrc, hash)
            ### TODO: What if the file is not cached?
            data = ''
            fd = None
            try:
                fd = open(path, 'rb')
                data = fd.read()
            finally:
                if fd:
                    fd.close()
            return context.memfilectx(f, data, 'l' in fctx.flags(),
                                      'x' in fctx.flags(), renamed)
        else:
            return _getnormalcontext(repo.ui, ctx, f, revmap)

    dstfiles = []
    for file in files:
        if lfutil.isstandin(file):
            dstfiles.append(lfutil.splitstandin(file))
        else:
            dstfiles.append(file)
    # Commit
    _commitcontext(rdst, parents, ctx, dstfiles, getfilectx, revmap)

def _lfconvert_addchangeset(rsrc, rdst, ctx, revmap, lfiles, normalfiles,
        matcher, size, lfiletohash):
    # Convert src parents to dst parents
    parents = _convertparents(ctx, revmap)

    # Generate list of changed files
    files = _getchangedfiles(ctx, parents)

    dstfiles = []
    for f in files:
        if f not in lfiles and f not in normalfiles:
            islfile = _islfile(f, ctx, matcher, size)
            # If this file was renamed or copied then copy
            # the lfileness of its predecessor
            if f in ctx.manifest():
                fctx = ctx.filectx(f)
                renamed = fctx.renamed()
                renamedlfile = renamed and renamed[0] in lfiles
                islfile |= renamedlfile
                if 'l' in fctx.flags():
                    if renamedlfile:
                        raise util.Abort(
                            _('renamed/copied largefile %s becomes symlink')
                            % f)
                    islfile = False
            if islfile:
                lfiles.add(f)
            else:
                normalfiles.add(f)

        if f in lfiles:
            dstfiles.append(lfutil.standin(f))
            # largefile in manifest if it has not been removed/renamed
            if f in ctx.manifest():
                fctx = ctx.filectx(f)
                if 'l' in fctx.flags():
                    renamed = fctx.renamed()
                    if renamed and renamed[0] in lfiles:
                        raise util.Abort(_('largefile %s becomes symlink') % f)

                # largefile was modified, update standins
                fullpath = rdst.wjoin(f)
                util.makedirs(os.path.dirname(fullpath))
                m = util.sha1('')
                m.update(ctx[f].data())
                hash = m.hexdigest()
                if f not in lfiletohash or lfiletohash[f] != hash:
                    try:
                        fd = open(fullpath, 'wb')
                        fd.write(ctx[f].data())
                    finally:
                        if fd:
                            fd.close()
                    executable = 'x' in ctx[f].flags()
                    os.chmod(fullpath, lfutil.getmode(executable))
                    lfutil.writestandin(rdst, lfutil.standin(f), hash,
                        executable)
                    lfiletohash[f] = hash
        else:
            # normal file
            dstfiles.append(f)

    def getfilectx(repo, memctx, f):
        if lfutil.isstandin(f):
            # if the file isn't in the manifest then it was removed
            # or renamed, raise IOError to indicate this
            srcfname = lfutil.splitstandin(f)
            try:
                fctx = ctx.filectx(srcfname)
            except error.LookupError:
                raise IOError()
            renamed = fctx.renamed()
            if renamed:
                # standin is always a largefile because largefile-ness
                # doesn't change after rename or copy
                renamed = lfutil.standin(renamed[0])

            return context.memfilectx(f, lfiletohash[srcfname] + '\n', 'l' in
                fctx.flags(), 'x' in fctx.flags(), renamed)
        else:
            return _getnormalcontext(repo.ui, ctx, f, revmap)

    # Commit
    _commitcontext(rdst, parents, ctx, dstfiles, getfilectx, revmap)

def _commitcontext(rdst, parents, ctx, dstfiles, getfilectx, revmap):
    mctx = context.memctx(rdst, parents, ctx.description(), dstfiles,
                          getfilectx, ctx.user(), ctx.date(), ctx.extra())
    ret = rdst.commitctx(mctx)
    rdst.dirstate.setparents(ret)
    revmap[ctx.node()] = rdst.changelog.tip()

# Generate list of changed files
def _getchangedfiles(ctx, parents):
    files = set(ctx.files())
    if node.nullid not in parents:
        mc = ctx.manifest()
        mp1 = ctx.parents()[0].manifest()
        mp2 = ctx.parents()[1].manifest()
        files |= (set(mp1) | set(mp2)) - set(mc)
        for f in mc:
            if mc[f] != mp1.get(f, None) or mc[f] != mp2.get(f, None):
                files.add(f)
    return files

# Convert src parents to dst parents
def _convertparents(ctx, revmap):
    parents = []
    for p in ctx.parents():
        parents.append(revmap[p.node()])
    while len(parents) < 2:
        parents.append(node.nullid)
    return parents

# Get memfilectx for a normal file
def _getnormalcontext(ui, ctx, f, revmap):
    try:
        fctx = ctx.filectx(f)
    except error.LookupError:
        raise IOError()
    renamed = fctx.renamed()
    if renamed:
        renamed = renamed[0]

    data = fctx.data()
    if f == '.hgtags':
        data = _converttags (ui, revmap, data)
    return context.memfilectx(f, data, 'l' in fctx.flags(),
                              'x' in fctx.flags(), renamed)

# Remap tag data using a revision map
def _converttags(ui, revmap, data):
    newdata = []
    for line in data.splitlines():
        try:
            id, name = line.split(' ', 1)
        except ValueError:
            ui.warn(_('skipping incorrectly formatted tag %s\n'
                % line))
            continue
        try:
            newid = node.bin(id)
        except TypeError:
            ui.warn(_('skipping incorrectly formatted id %s\n'
                % id))
            continue
        try:
            newdata.append('%s %s\n' % (node.hex(revmap[newid]),
                name))
        except KeyError:
            ui.warn(_('no mapping for id %s\n') % id)
            continue
    return ''.join(newdata)

def _islfile(file, ctx, matcher, size):
    '''Return true if file should be considered a largefile, i.e.
    matcher matches it or it is larger than size.'''
    # never store special .hg* files as largefiles
    if file == '.hgtags' or file == '.hgignore' or file == '.hgsigs':
        return False
    if matcher and matcher(file):
        return True
    try:
        return ctx.filectx(file).size() >= size * 1024 * 1024
    except error.LookupError:
        return False

def uploadlfiles(ui, rsrc, rdst, files):
    '''upload largefiles to the central store'''

    if not files:
        return

    store = basestore._openstore(rsrc, rdst, put=True)

    at = 0
    files = filter(lambda h: not store.exists(h), files)
    for hash in files:
        ui.progress(_('uploading largefiles'), at, unit='largefile',
                    total=len(files))
        source = lfutil.findfile(rsrc, hash)
        if not source:
            raise util.Abort(_('largefile %s missing from store'
                               ' (needs to be uploaded)') % hash)
        # XXX check for errors here
        store.put(source, hash)
        at += 1
    ui.progress(_('uploading largefiles'), None)

def verifylfiles(ui, repo, all=False, contents=False):
    '''Verify that every big file revision in the current changeset
    exists in the central store.  With --contents, also verify that
    the contents of each big file revision are correct (SHA-1 hash
    matches the revision ID).  With --all, check every changeset in
    this repository.'''
    if all:
        # Pass a list to the function rather than an iterator because we know a
        # list will work.
        revs = range(len(repo))
    else:
        revs = ['.']

    store = basestore._openstore(repo)
    return store.verify(revs, contents=contents)

def cachelfiles(ui, repo, node):
    '''cachelfiles ensures that all largefiles needed by the specified revision
    are present in the repository's largefile cache.

    returns a tuple (cached, missing).  cached is the list of files downloaded
    by this operation; missing is the list of files that were needed but could
    not be found.'''
    lfiles = lfutil.listlfiles(repo, node)
    toget = []

    for lfile in lfiles:
        # If we are mid-merge, then we have to trust the standin that is in the
        # working copy to have the correct hashvalue.  This is because the
        # original hg.merge() already updated the standin as part of the normal
        # merge process -- we just have to udpate the largefile to match.
        if (getattr(repo, "_ismerging", False) and
             os.path.exists(repo.wjoin(lfutil.standin(lfile)))):
            expectedhash = lfutil.readstandin(repo, lfile)
        else:
            expectedhash = repo[node][lfutil.standin(lfile)].data().strip()

        # if it exists and its hash matches, it might have been locally
        # modified before updating and the user chose 'local'.  in this case,
        # it will not be in any store, so don't look for it.
        if ((not os.path.exists(repo.wjoin(lfile)) or
             expectedhash != lfutil.hashfile(repo.wjoin(lfile))) and
            not lfutil.findfile(repo, expectedhash)):
            toget.append((lfile, expectedhash))

    if toget:
        store = basestore._openstore(repo)
        ret = store.get(toget)
        return ret

    return ([], [])

def updatelfiles(ui, repo, filelist=None, printmessage=True):
    wlock = repo.wlock()
    try:
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        lfiles = set(lfutil.listlfiles(repo)) | set(lfdirstate)

        if filelist is not None:
            lfiles = [f for f in lfiles if f in filelist]

        printed = False
        if printmessage and lfiles:
            ui.status(_('getting changed largefiles\n'))
            printed = True
            cachelfiles(ui, repo, '.')

        updated, removed = 0, 0
        for i in map(lambda f: _updatelfile(repo, lfdirstate, f), lfiles):
            # increment the appropriate counter according to _updatelfile's
            # return value
            updated += i > 0 and i or 0
            removed -= i < 0 and i or 0
            if printmessage and (removed or updated) and not printed:
                ui.status(_('getting changed largefiles\n'))
                printed = True

        lfdirstate.write()
        if printed and printmessage:
            ui.status(_('%d largefiles updated, %d removed\n') % (updated,
                removed))
    finally:
        wlock.release()

def _updatelfile(repo, lfdirstate, lfile):
    '''updates a single largefile and copies the state of its standin from
    the repository's dirstate to its state in the lfdirstate.

    returns 1 if the file was modified, -1 if the file was removed, 0 if the
    file was unchanged, and None if the needed largefile was missing from the
    cache.'''
    ret = 0
    abslfile = repo.wjoin(lfile)
    absstandin = repo.wjoin(lfutil.standin(lfile))
    if os.path.exists(absstandin):
        if os.path.exists(absstandin+'.orig'):
            shutil.copyfile(abslfile, abslfile+'.orig')
        expecthash = lfutil.readstandin(repo, lfile)
        if (expecthash != '' and
            (not os.path.exists(abslfile) or
             expecthash != lfutil.hashfile(abslfile))):
            if not lfutil.copyfromcache(repo, expecthash, lfile):
                # use normallookup() to allocate entry in largefiles dirstate,
                # because lack of it misleads lfiles_repo.status() into
                # recognition that such cache missing files are REMOVED.
                lfdirstate.normallookup(lfile)
                return None # don't try to set the mode
            ret = 1
        mode = os.stat(absstandin).st_mode
        if mode != os.stat(abslfile).st_mode:
            os.chmod(abslfile, mode)
            ret = 1
    else:
        # Remove lfiles for which the standin is deleted, unless the
        # lfile is added to the repository again. This happens when a
        # largefile is converted back to a normal file: the standin
        # disappears, but a new (normal) file appears as the lfile.
        if os.path.exists(abslfile) and lfile not in repo[None]:
            util.unlinkpath(abslfile)
            ret = -1
    state = repo.dirstate[lfutil.standin(lfile)]
    if state == 'n':
        # When rebasing, we need to synchronize the standin and the largefile,
        # because otherwise the largefile will get reverted.  But for commit's
        # sake, we have to mark the file as unclean.
        if getattr(repo, "_isrebasing", False):
           lfdirstate.normallookup(lfile)
        else:
            lfdirstate.normal(lfile)
    elif state == 'r':
        lfdirstate.remove(lfile)
    elif state == 'a':
        lfdirstate.add(lfile)
    elif state == '?':
        lfdirstate.drop(lfile)
    return ret

# -- hg commands declarations ------------------------------------------------

cmdtable = {
    'lfconvert': (lfconvert,
                  [('s', 'size', '',
                    _('minimum size (MB) for files to be converted '
                      'as largefiles'),
                    'SIZE'),
                  ('', 'to-normal', False,
                   _('convert from a largefiles repo to a normal repo')),
                  ],
                  _('hg lfconvert SOURCE DEST [FILE ...]')),
    }
