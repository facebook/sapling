# Copyright 2009-2010 Gregory P. Ward
# Copyright 2009-2010 Intelerad Medical Systems Incorporated
# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''Overridden Mercurial commands and functions for the largefiles extension'''

import os
import copy

from mercurial import hg, commands, util, cmdutil, scmutil, match as match_, \
        archival, pathutil, revset
from mercurial.i18n import _
from mercurial.node import hex
from hgext import rebase

import lfutil
import lfcommands
import basestore

# -- Utility functions: commonly/repeatedly needed functionality ---------------

def installnormalfilesmatchfn(manifest):
    '''installmatchfn with a matchfn that ignores all largefiles'''
    def overridematch(ctx, pats=[], opts={}, globbed=False,
            default='relpath'):
        match = oldmatch(ctx, pats, opts, globbed, default)
        m = copy.copy(match)
        notlfile = lambda f: not (lfutil.isstandin(f) or lfutil.standin(f) in
                manifest)
        m._files = filter(notlfile, m._files)
        m._fmap = set(m._files)
        m._always = False
        origmatchfn = m.matchfn
        m.matchfn = lambda f: notlfile(f) and origmatchfn(f) or None
        return m
    oldmatch = installmatchfn(overridematch)

def installmatchfn(f):
    '''monkey patch the scmutil module with a custom match function.
    Warning: it is monkey patching the _module_ on runtime! Not thread safe!'''
    oldmatch = scmutil.match
    setattr(f, 'oldmatch', oldmatch)
    scmutil.match = f
    return oldmatch

def restorematchfn():
    '''restores scmutil.match to what it was before installmatchfn
    was called.  no-op if scmutil.match is its original function.

    Note that n calls to installmatchfn will require n calls to
    restore matchfn to reverse'''
    scmutil.match = getattr(scmutil.match, 'oldmatch')

def installmatchandpatsfn(f):
    oldmatchandpats = scmutil.matchandpats
    setattr(f, 'oldmatchandpats', oldmatchandpats)
    scmutil.matchandpats = f
    return oldmatchandpats

def restorematchandpatsfn():
    '''restores scmutil.matchandpats to what it was before
    installnormalfilesmatchandpatsfn was called.  no-op if scmutil.matchandpats
    is its original function.

    Note that n calls to installnormalfilesmatchandpatsfn will require n calls
    to restore matchfn to reverse'''
    scmutil.matchandpats = getattr(scmutil.matchandpats, 'oldmatchandpats',
            scmutil.matchandpats)

def addlargefiles(ui, repo, *pats, **opts):
    large = opts.pop('large', None)
    lfsize = lfutil.getminsize(
        ui, lfutil.islfilesrepo(repo), opts.pop('lfsize', None))

    lfmatcher = None
    if lfutil.islfilesrepo(repo):
        lfpats = ui.configlist(lfutil.longname, 'patterns', default=[])
        if lfpats:
            lfmatcher = match_.match(repo.root, '', list(lfpats))

    lfnames = []
    m = scmutil.match(repo[None], pats, opts)
    m.bad = lambda x, y: None
    wctx = repo[None]
    for f in repo.walk(m):
        exact = m.exact(f)
        lfile = lfutil.standin(f) in wctx
        nfile = f in wctx
        exists = lfile or nfile

        # Don't warn the user when they attempt to add a normal tracked file.
        # The normal add code will do that for us.
        if exact and exists:
            if lfile:
                ui.warn(_('%s already a largefile\n') % f)
            continue

        if (exact or not exists) and not lfutil.isstandin(f):
            wfile = repo.wjoin(f)

            # In case the file was removed previously, but not committed
            # (issue3507)
            if not os.path.exists(wfile):
                continue

            abovemin = (lfsize and
                        os.lstat(wfile).st_size >= lfsize * 1024 * 1024)
            if large or abovemin or (lfmatcher and lfmatcher(f)):
                lfnames.append(f)
                if ui.verbose or not exact:
                    ui.status(_('adding %s as a largefile\n') % m.rel(f))

    bad = []
    standins = []

    # Need to lock, otherwise there could be a race condition between
    # when standins are created and added to the repo.
    wlock = repo.wlock()
    try:
        if not opts.get('dry_run'):
            lfdirstate = lfutil.openlfdirstate(ui, repo)
            for f in lfnames:
                standinname = lfutil.standin(f)
                lfutil.writestandin(repo, standinname, hash='',
                    executable=lfutil.getexecutable(repo.wjoin(f)))
                standins.append(standinname)
                if lfdirstate[f] == 'r':
                    lfdirstate.normallookup(f)
                else:
                    lfdirstate.add(f)
            lfdirstate.write()
            bad += [lfutil.splitstandin(f)
                    for f in repo[None].add(standins)
                    if f in m.files()]
    finally:
        wlock.release()
    return bad

def removelargefiles(ui, repo, *pats, **opts):
    after = opts.get('after')
    if not pats and not after:
        raise util.Abort(_('no files specified'))
    m = scmutil.match(repo[None], pats, opts)
    try:
        repo.lfstatus = True
        s = repo.status(match=m, clean=True)
    finally:
        repo.lfstatus = False
    manifest = repo[None].manifest()
    modified, added, deleted, clean = [[f for f in list
                                        if lfutil.standin(f) in manifest]
                                       for list in [s[0], s[1], s[3], s[6]]]

    def warn(files, msg):
        for f in files:
            ui.warn(msg % m.rel(f))
        return int(len(files) > 0)

    result = 0

    if after:
        remove, forget = deleted, []
        result = warn(modified + added + clean,
                      _('not removing %s: file still exists\n'))
    else:
        remove, forget = deleted + clean, []
        result = warn(modified, _('not removing %s: file is modified (use -f'
                                  ' to force removal)\n'))
        result = warn(added, _('not removing %s: file has been marked for add'
                               ' (use forget to undo)\n')) or result

    for f in sorted(remove + forget):
        if ui.verbose or not m.exact(f):
            ui.status(_('removing %s\n') % m.rel(f))

    # Need to lock because standin files are deleted then removed from the
    # repository and we could race in-between.
    wlock = repo.wlock()
    try:
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        for f in remove:
            if not after:
                # If this is being called by addremove, notify the user that we
                # are removing the file.
                if getattr(repo, "_isaddremove", False):
                    ui.status(_('removing %s\n') % f)
                util.unlinkpath(repo.wjoin(f), ignoremissing=True)
            lfdirstate.remove(f)
        lfdirstate.write()
        forget = [lfutil.standin(f) for f in forget]
        remove = [lfutil.standin(f) for f in remove]
        repo[None].forget(forget)
        # If this is being called by addremove, let the original addremove
        # function handle this.
        if not getattr(repo, "_isaddremove", False):
            for f in remove:
                util.unlinkpath(repo.wjoin(f), ignoremissing=True)
        repo[None].forget(remove)
    finally:
        wlock.release()

    return result

# For overriding mercurial.hgweb.webcommands so that largefiles will
# appear at their right place in the manifests.
def decodepath(orig, path):
    return lfutil.splitstandin(path) or path

# -- Wrappers: modify existing commands --------------------------------

# Add works by going through the files that the user wanted to add and
# checking if they should be added as largefiles. Then it makes a new
# matcher which matches only the normal files and runs the original
# version of add.
def overrideadd(orig, ui, repo, *pats, **opts):
    normal = opts.pop('normal')
    if normal:
        if opts.get('large'):
            raise util.Abort(_('--normal cannot be used with --large'))
        return orig(ui, repo, *pats, **opts)
    bad = addlargefiles(ui, repo, *pats, **opts)
    installnormalfilesmatchfn(repo[None].manifest())
    result = orig(ui, repo, *pats, **opts)
    restorematchfn()

    return (result == 1 or bad) and 1 or 0

def overrideremove(orig, ui, repo, *pats, **opts):
    installnormalfilesmatchfn(repo[None].manifest())
    result = orig(ui, repo, *pats, **opts)
    restorematchfn()
    return removelargefiles(ui, repo, *pats, **opts) or result

def overridestatusfn(orig, repo, rev2, **opts):
    try:
        repo._repo.lfstatus = True
        return orig(repo, rev2, **opts)
    finally:
        repo._repo.lfstatus = False

def overridestatus(orig, ui, repo, *pats, **opts):
    try:
        repo.lfstatus = True
        return orig(ui, repo, *pats, **opts)
    finally:
        repo.lfstatus = False

def overridedirty(orig, repo, ignoreupdate=False):
    try:
        repo._repo.lfstatus = True
        return orig(repo, ignoreupdate)
    finally:
        repo._repo.lfstatus = False

def overridelog(orig, ui, repo, *pats, **opts):
    def overridematchandpats(ctx, pats=[], opts={}, globbed=False,
            default='relpath'):
        """Matcher that merges root directory with .hglf, suitable for log.
        It is still possible to match .hglf directly.
        For any listed files run log on the standin too.
        matchfn tries both the given filename and with .hglf stripped.
        """
        matchandpats = oldmatchandpats(ctx, pats, opts, globbed, default)
        m, p = copy.copy(matchandpats)

        if m.always():
            # We want to match everything anyway, so there's no benefit trying
            # to add standins.
            return matchandpats

        pats = set(p)
        # TODO: handling of patterns in both cases below
        if m._cwd:
            if os.path.isabs(m._cwd):
                # TODO: handle largefile magic when invoked from other cwd
                return matchandpats
            back = (m._cwd.count('/') + 1) * '../'
            pats.update(back + lfutil.standin(m._cwd + '/' + f) for f in p)
        else:
            pats.update(lfutil.standin(f) for f in p)

        for i in range(0, len(m._files)):
            standin = lfutil.standin(m._files[i])
            if standin in repo[ctx.node()]:
                m._files[i] = standin
            elif m._files[i] not in repo[ctx.node()]:
                m._files.append(standin)
            pats.add(standin)

        m._fmap = set(m._files)
        m._always = False
        origmatchfn = m.matchfn
        def lfmatchfn(f):
            lf = lfutil.splitstandin(f)
            if lf is not None and origmatchfn(lf):
                return True
            r = origmatchfn(f)
            return r
        m.matchfn = lfmatchfn

        return m, pats

    # For hg log --patch, the match object is used in two different senses:
    # (1) to determine what revisions should be printed out, and
    # (2) to determine what files to print out diffs for.
    # The magic matchandpats override should be used for case (1) but not for
    # case (2).
    def overridemakelogfilematcher(repo, pats, opts):
        pctx = repo[None]
        match, pats = oldmatchandpats(pctx, pats, opts)
        return lambda rev: match

    oldmatchandpats = installmatchandpatsfn(overridematchandpats)
    oldmakelogfilematcher = cmdutil._makenofollowlogfilematcher
    setattr(cmdutil, '_makenofollowlogfilematcher', overridemakelogfilematcher)

    try:
        return orig(ui, repo, *pats, **opts)
    finally:
        restorematchandpatsfn()
        setattr(cmdutil, '_makenofollowlogfilematcher', oldmakelogfilematcher)

def overrideverify(orig, ui, repo, *pats, **opts):
    large = opts.pop('large', False)
    all = opts.pop('lfa', False)
    contents = opts.pop('lfc', False)

    result = orig(ui, repo, *pats, **opts)
    if large or all or contents:
        result = result or lfcommands.verifylfiles(ui, repo, all, contents)
    return result

def overridedebugstate(orig, ui, repo, *pats, **opts):
    large = opts.pop('large', False)
    if large:
        class fakerepo(object):
            dirstate = lfutil.openlfdirstate(ui, repo)
        orig(ui, fakerepo, *pats, **opts)
    else:
        orig(ui, repo, *pats, **opts)

# Override needs to refresh standins so that update's normal merge
# will go through properly. Then the other update hook (overriding repo.update)
# will get the new files. Filemerge is also overridden so that the merge
# will merge standins correctly.
def overrideupdate(orig, ui, repo, *pats, **opts):
    # Need to lock between the standins getting updated and their
    # largefiles getting updated
    wlock = repo.wlock()
    try:
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        s = lfdirstate.status(match_.always(repo.root, repo.getcwd()),
            [], False, False, False)
        (unsure, modified, added, removed, missing, unknown, ignored, clean) = s

        if opts['check']:
            mod = len(modified) > 0
            for lfile in unsure:
                standin = lfutil.standin(lfile)
                if repo['.'][standin].data().strip() != \
                        lfutil.hashfile(repo.wjoin(lfile)):
                    mod = True
                else:
                    lfdirstate.normal(lfile)
            lfdirstate.write()
            if mod:
                raise util.Abort(_('uncommitted changes'))
        return orig(ui, repo, *pats, **opts)
    finally:
        wlock.release()

# Before starting the manifest merge, merge.updates will call
# _checkunknown to check if there are any files in the merged-in
# changeset that collide with unknown files in the working copy.
#
# The largefiles are seen as unknown, so this prevents us from merging
# in a file 'foo' if we already have a largefile with the same name.
#
# The overridden function filters the unknown files by removing any
# largefiles. This makes the merge proceed and we can then handle this
# case further in the overridden manifestmerge function below.
def overridecheckunknownfile(origfn, repo, wctx, mctx, f):
    if lfutil.standin(repo.dirstate.normalize(f)) in wctx:
        return False
    return origfn(repo, wctx, mctx, f)

# The manifest merge handles conflicts on the manifest level. We want
# to handle changes in largefile-ness of files at this level too.
#
# The strategy is to run the original manifestmerge and then process
# the action list it outputs. There are two cases we need to deal with:
#
# 1. Normal file in p1, largefile in p2. Here the largefile is
#    detected via its standin file, which will enter the working copy
#    with a "get" action. It is not "merge" since the standin is all
#    Mercurial is concerned with at this level -- the link to the
#    existing normal file is not relevant here.
#
# 2. Largefile in p1, normal file in p2. Here we get a "merge" action
#    since the largefile will be present in the working copy and
#    different from the normal file in p2. Mercurial therefore
#    triggers a merge action.
#
# In both cases, we prompt the user and emit new actions to either
# remove the standin (if the normal file was kept) or to remove the
# normal file and get the standin (if the largefile was kept). The
# default prompt answer is to use the largefile version since it was
# presumably changed on purpose.
#
# Finally, the merge.applyupdates function will then take care of
# writing the files into the working copy and lfcommands.updatelfiles
# will update the largefiles.
def overridecalculateupdates(origfn, repo, p1, p2, pas, branchmerge, force,
                             partial, acceptremote, followcopies):
    overwrite = force and not branchmerge
    actions = origfn(repo, p1, p2, pas, branchmerge, force, partial,
                     acceptremote, followcopies)

    if overwrite:
        return actions

    removes = set(a[0] for a in actions['r'])

    newglist = []
    lfmr = [] # LargeFiles: Mark as Removed
    for action in actions['g']:
        f, args, msg = action
        splitstandin = f and lfutil.splitstandin(f)
        if (splitstandin is not None and
            splitstandin in p1 and splitstandin not in removes):
            # Case 1: normal file in the working copy, largefile in
            # the second parent
            lfile = splitstandin
            standin = f
            msg = _('remote turned local normal file %s into a largefile\n'
                    'use (l)argefile or keep (n)ormal file?'
                    '$$ &Largefile $$ &Normal file') % lfile
            if repo.ui.promptchoice(msg, 0) == 0:
                actions['r'].append((lfile, None, msg))
                newglist.append((standin, (p2.flags(standin),), msg))
            else:
                actions['r'].append((standin, None, msg))
        elif lfutil.standin(f) in p1 and lfutil.standin(f) not in removes:
            # Case 2: largefile in the working copy, normal file in
            # the second parent
            standin = lfutil.standin(f)
            lfile = f
            msg = _('remote turned local largefile %s into a normal file\n'
                    'keep (l)argefile or use (n)ormal file?'
                    '$$ &Largefile $$ &Normal file') % lfile
            if repo.ui.promptchoice(msg, 0) == 0:
                if branchmerge:
                    # largefile can be restored from standin safely
                    actions['r'].append((lfile, None, msg))
                else:
                    # "lfile" should be marked as "removed" without
                    # removal of itself
                    lfmr.append((lfile, None, msg))

                    # linear-merge should treat this largefile as 're-added'
                    actions['a'].append((standin, None, msg))
            else:
                actions['r'].append((standin, None, msg))
                newglist.append((lfile, (p2.flags(lfile),), msg))
        else:
            newglist.append(action)

    newglist.sort()
    actions['g'] = newglist
    if lfmr:
        lfmr.sort()
        actions['lfmr'] = lfmr

    return actions

def mergerecordupdates(orig, repo, actions, branchmerge):
    if 'lfmr' in actions:
        # this should be executed before 'orig', to execute 'remove'
        # before all other actions
        for lfile, args, msg in actions['lfmr']:
            repo.dirstate.remove(lfile)

    return orig(repo, actions, branchmerge)


# Override filemerge to prompt the user about how they wish to merge
# largefiles. This will handle identical edits without prompting the user.
def overridefilemerge(origfn, repo, mynode, orig, fcd, fco, fca, labels=None):
    if not lfutil.isstandin(orig):
        return origfn(repo, mynode, orig, fcd, fco, fca, labels=labels)

    ahash = fca.data().strip().lower()
    dhash = fcd.data().strip().lower()
    ohash = fco.data().strip().lower()
    if (ohash != ahash and
        ohash != dhash and
        (dhash == ahash or
         repo.ui.promptchoice(
             _('largefile %s has a merge conflict\nancestor was %s\n'
               'keep (l)ocal %s or\ntake (o)ther %s?'
               '$$ &Local $$ &Other') %
               (lfutil.splitstandin(orig), ahash, dhash, ohash),
             0) == 1)):
        repo.wwrite(fcd.path(), fco.data(), fco.flags())
    return 0

# Copy first changes the matchers to match standins instead of
# largefiles.  Then it overrides util.copyfile in that function it
# checks if the destination largefile already exists. It also keeps a
# list of copied files so that the largefiles can be copied and the
# dirstate updated.
def overridecopy(orig, ui, repo, pats, opts, rename=False):
    # doesn't remove largefile on rename
    if len(pats) < 2:
        # this isn't legal, let the original function deal with it
        return orig(ui, repo, pats, opts, rename)

    def makestandin(relpath):
        path = pathutil.canonpath(repo.root, repo.getcwd(), relpath)
        return os.path.join(repo.wjoin(lfutil.standin(path)))

    fullpats = scmutil.expandpats(pats)
    dest = fullpats[-1]

    if os.path.isdir(dest):
        if not os.path.isdir(makestandin(dest)):
            os.makedirs(makestandin(dest))
    # This could copy both lfiles and normal files in one command,
    # but we don't want to do that. First replace their matcher to
    # only match normal files and run it, then replace it to just
    # match largefiles and run it again.
    nonormalfiles = False
    nolfiles = False
    installnormalfilesmatchfn(repo[None].manifest())
    try:
        try:
            result = orig(ui, repo, pats, opts, rename)
        except util.Abort, e:
            if str(e) != _('no files to copy'):
                raise e
            else:
                nonormalfiles = True
            result = 0
    finally:
        restorematchfn()

    # The first rename can cause our current working directory to be removed.
    # In that case there is nothing left to copy/rename so just quit.
    try:
        repo.getcwd()
    except OSError:
        return result

    try:
        try:
            # When we call orig below it creates the standins but we don't add
            # them to the dir state until later so lock during that time.
            wlock = repo.wlock()

            manifest = repo[None].manifest()
            def overridematch(ctx, pats=[], opts={}, globbed=False,
                    default='relpath'):
                newpats = []
                # The patterns were previously mangled to add the standin
                # directory; we need to remove that now
                for pat in pats:
                    if match_.patkind(pat) is None and lfutil.shortname in pat:
                        newpats.append(pat.replace(lfutil.shortname, ''))
                    else:
                        newpats.append(pat)
                match = oldmatch(ctx, newpats, opts, globbed, default)
                m = copy.copy(match)
                lfile = lambda f: lfutil.standin(f) in manifest
                m._files = [lfutil.standin(f) for f in m._files if lfile(f)]
                m._fmap = set(m._files)
                m._always = False
                origmatchfn = m.matchfn
                m.matchfn = lambda f: (lfutil.isstandin(f) and
                                    (f in manifest) and
                                    origmatchfn(lfutil.splitstandin(f)) or
                                    None)
                return m
            oldmatch = installmatchfn(overridematch)
            listpats = []
            for pat in pats:
                if match_.patkind(pat) is not None:
                    listpats.append(pat)
                else:
                    listpats.append(makestandin(pat))

            try:
                origcopyfile = util.copyfile
                copiedfiles = []
                def overridecopyfile(src, dest):
                    if (lfutil.shortname in src and
                        dest.startswith(repo.wjoin(lfutil.shortname))):
                        destlfile = dest.replace(lfutil.shortname, '')
                        if not opts['force'] and os.path.exists(destlfile):
                            raise IOError('',
                                _('destination largefile already exists'))
                    copiedfiles.append((src, dest))
                    origcopyfile(src, dest)

                util.copyfile = overridecopyfile
                result += orig(ui, repo, listpats, opts, rename)
            finally:
                util.copyfile = origcopyfile

            lfdirstate = lfutil.openlfdirstate(ui, repo)
            for (src, dest) in copiedfiles:
                if (lfutil.shortname in src and
                    dest.startswith(repo.wjoin(lfutil.shortname))):
                    srclfile = src.replace(repo.wjoin(lfutil.standin('')), '')
                    destlfile = dest.replace(repo.wjoin(lfutil.standin('')), '')
                    destlfiledir = os.path.dirname(repo.wjoin(destlfile)) or '.'
                    if not os.path.isdir(destlfiledir):
                        os.makedirs(destlfiledir)
                    if rename:
                        os.rename(repo.wjoin(srclfile), repo.wjoin(destlfile))

                        # The file is gone, but this deletes any empty parent
                        # directories as a side-effect.
                        util.unlinkpath(repo.wjoin(srclfile), True)
                        lfdirstate.remove(srclfile)
                    else:
                        util.copyfile(repo.wjoin(srclfile),
                                      repo.wjoin(destlfile))

                    lfdirstate.add(destlfile)
            lfdirstate.write()
        except util.Abort, e:
            if str(e) != _('no files to copy'):
                raise e
            else:
                nolfiles = True
    finally:
        restorematchfn()
        wlock.release()

    if nolfiles and nonormalfiles:
        raise util.Abort(_('no files to copy'))

    return result

# When the user calls revert, we have to be careful to not revert any
# changes to other largefiles accidentally. This means we have to keep
# track of the largefiles that are being reverted so we only pull down
# the necessary largefiles.
#
# Standins are only updated (to match the hash of largefiles) before
# commits. Update the standins then run the original revert, changing
# the matcher to hit standins instead of largefiles. Based on the
# resulting standins update the largefiles.
def overriderevert(orig, ui, repo, *pats, **opts):
    # Because we put the standins in a bad state (by updating them)
    # and then return them to a correct state we need to lock to
    # prevent others from changing them in their incorrect state.
    wlock = repo.wlock()
    try:
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        (modified, added, removed, missing, unknown, ignored, clean) = \
            lfutil.lfdirstatestatus(lfdirstate, repo, repo['.'].rev())
        lfdirstate.write()
        for lfile in modified:
            lfutil.updatestandin(repo, lfutil.standin(lfile))
        for lfile in missing:
            if (os.path.exists(repo.wjoin(lfutil.standin(lfile)))):
                os.unlink(repo.wjoin(lfutil.standin(lfile)))

        oldstandins = lfutil.getstandinsstate(repo)

        def overridematch(ctx, pats=[], opts={}, globbed=False,
                default='relpath'):
            match = oldmatch(ctx, pats, opts, globbed, default)
            m = copy.copy(match)
            def tostandin(f):
                if lfutil.standin(f) in ctx:
                    return lfutil.standin(f)
                elif lfutil.standin(f) in repo[None]:
                    return None
                return f
            m._files = [tostandin(f) for f in m._files]
            m._files = [f for f in m._files if f is not None]
            m._fmap = set(m._files)
            m._always = False
            origmatchfn = m.matchfn
            def matchfn(f):
                if lfutil.isstandin(f):
                    return (origmatchfn(lfutil.splitstandin(f)) and
                            (f in repo[None] or f in ctx))
                return origmatchfn(f)
            m.matchfn = matchfn
            return m
        oldmatch = installmatchfn(overridematch)
        try:
            orig(ui, repo, *pats, **opts)
        finally:
            restorematchfn()

        newstandins = lfutil.getstandinsstate(repo)
        filelist = lfutil.getlfilestoupdate(oldstandins, newstandins)
        # lfdirstate should be 'normallookup'-ed for updated files,
        # because reverting doesn't touch dirstate for 'normal' files
        # when target revision is explicitly specified: in such case,
        # 'n' and valid timestamp in dirstate doesn't ensure 'clean'
        # of target (standin) file.
        lfcommands.updatelfiles(ui, repo, filelist, printmessage=False,
                                normallookup=True)

    finally:
        wlock.release()

# When we rebase a repository with remotely changed largefiles, we need to
# take some extra care so that the largefiles are correctly updated in the
# working copy
def overridepull(orig, ui, repo, source=None, **opts):
    revsprepull = len(repo)
    if not source:
        source = 'default'
    repo.lfpullsource = source
    if opts.get('rebase', False):
        repo._isrebasing = True
        try:
            if opts.get('update'):
                del opts['update']
                ui.debug('--update and --rebase are not compatible, ignoring '
                         'the update flag\n')
            del opts['rebase']
            origpostincoming = commands.postincoming
            def _dummy(*args, **kwargs):
                pass
            commands.postincoming = _dummy
            try:
                result = commands.pull(ui, repo, source, **opts)
            finally:
                commands.postincoming = origpostincoming
            revspostpull = len(repo)
            if revspostpull > revsprepull:
                result = result or rebase.rebase(ui, repo)
        finally:
            repo._isrebasing = False
    else:
        result = orig(ui, repo, source, **opts)
    revspostpull = len(repo)
    lfrevs = opts.get('lfrev', [])
    if opts.get('all_largefiles'):
        lfrevs.append('pulled()')
    if lfrevs and revspostpull > revsprepull:
        numcached = 0
        repo.firstpulled = revsprepull # for pulled() revset expression
        try:
            for rev in scmutil.revrange(repo, lfrevs):
                ui.note(_('pulling largefiles for revision %s\n') % rev)
                (cached, missing) = lfcommands.cachelfiles(ui, repo, rev)
                numcached += len(cached)
        finally:
            del repo.firstpulled
        ui.status(_("%d largefiles cached\n") % numcached)
    return result

def pulledrevsetsymbol(repo, subset, x):
    """``pulled()``
    Changesets that just has been pulled.

    Only available with largefiles from pull --lfrev expressions.

    .. container:: verbose

      Some examples:

      - pull largefiles for all new changesets::

          hg pull -lfrev "pulled()"

      - pull largefiles for all new branch heads::

          hg pull -lfrev "head(pulled()) and not closed()"

    """

    try:
        firstpulled = repo.firstpulled
    except AttributeError:
        raise util.Abort(_("pulled() only available in --lfrev"))
    return revset.baseset([r for r in subset if r >= firstpulled])

def overrideclone(orig, ui, source, dest=None, **opts):
    d = dest
    if d is None:
        d = hg.defaultdest(source)
    if opts.get('all_largefiles') and not hg.islocal(d):
            raise util.Abort(_(
            '--all-largefiles is incompatible with non-local destination %s') %
            d)

    return orig(ui, source, dest, **opts)

def hgclone(orig, ui, opts, *args, **kwargs):
    result = orig(ui, opts, *args, **kwargs)

    if result is not None:
        sourcerepo, destrepo = result
        repo = destrepo.local()

        # Caching is implicitly limited to 'rev' option, since the dest repo was
        # truncated at that point.  The user may expect a download count with
        # this option, so attempt whether or not this is a largefile repo.
        if opts.get('all_largefiles'):
            success, missing = lfcommands.downloadlfiles(ui, repo, None)

            if missing != 0:
                return None

    return result

def overriderebase(orig, ui, repo, **opts):
    repo._isrebasing = True
    try:
        return orig(ui, repo, **opts)
    finally:
        repo._isrebasing = False

def overridearchive(orig, repo, dest, node, kind, decode=True, matchfn=None,
            prefix=None, mtime=None, subrepos=None):
    # No need to lock because we are only reading history and
    # largefile caches, neither of which are modified.
    lfcommands.cachelfiles(repo.ui, repo, node)

    if kind not in archival.archivers:
        raise util.Abort(_("unknown archive type '%s'") % kind)

    ctx = repo[node]

    if kind == 'files':
        if prefix:
            raise util.Abort(
                _('cannot give prefix when archiving to files'))
    else:
        prefix = archival.tidyprefix(dest, kind, prefix)

    def write(name, mode, islink, getdata):
        if matchfn and not matchfn(name):
            return
        data = getdata()
        if decode:
            data = repo.wwritedata(name, data)
        archiver.addfile(prefix + name, mode, islink, data)

    archiver = archival.archivers[kind](dest, mtime or ctx.date()[0])

    if repo.ui.configbool("ui", "archivemeta", True):
        def metadata():
            base = 'repo: %s\nnode: %s\nbranch: %s\n' % (
                hex(repo.changelog.node(0)), hex(node), ctx.branch())

            tags = ''.join('tag: %s\n' % t for t in ctx.tags()
                           if repo.tagtype(t) == 'global')
            if not tags:
                repo.ui.pushbuffer()
                opts = {'template': '{latesttag}\n{latesttagdistance}',
                        'style': '', 'patch': None, 'git': None}
                cmdutil.show_changeset(repo.ui, repo, opts).show(ctx)
                ltags, dist = repo.ui.popbuffer().split('\n')
                tags = ''.join('latesttag: %s\n' % t for t in ltags.split(':'))
                tags += 'latesttagdistance: %s\n' % dist

            return base + tags

        write('.hg_archival.txt', 0644, False, metadata)

    for f in ctx:
        ff = ctx.flags(f)
        getdata = ctx[f].data
        if lfutil.isstandin(f):
            path = lfutil.findfile(repo, getdata().strip())
            if path is None:
                raise util.Abort(
                    _('largefile %s not found in repo store or system cache')
                    % lfutil.splitstandin(f))
            f = lfutil.splitstandin(f)

            def getdatafn():
                fd = None
                try:
                    fd = open(path, 'rb')
                    return fd.read()
                finally:
                    if fd:
                        fd.close()

            getdata = getdatafn
        write(f, 'x' in ff and 0755 or 0644, 'l' in ff, getdata)

    if subrepos:
        for subpath in sorted(ctx.substate):
            sub = ctx.sub(subpath)
            submatch = match_.narrowmatcher(subpath, matchfn)
            sub.archive(repo.ui, archiver, prefix, submatch)

    archiver.done()

def hgsubrepoarchive(orig, repo, ui, archiver, prefix, match=None):
    repo._get(repo._state + ('hg',))
    rev = repo._state[1]
    ctx = repo._repo[rev]

    lfcommands.cachelfiles(ui, repo._repo, ctx.node())

    def write(name, mode, islink, getdata):
        # At this point, the standin has been replaced with the largefile name,
        # so the normal matcher works here without the lfutil variants.
        if match and not match(f):
            return
        data = getdata()

        archiver.addfile(prefix + repo._path + '/' + name, mode, islink, data)

    for f in ctx:
        ff = ctx.flags(f)
        getdata = ctx[f].data
        if lfutil.isstandin(f):
            path = lfutil.findfile(repo._repo, getdata().strip())
            if path is None:
                raise util.Abort(
                    _('largefile %s not found in repo store or system cache')
                    % lfutil.splitstandin(f))
            f = lfutil.splitstandin(f)

            def getdatafn():
                fd = None
                try:
                    fd = open(os.path.join(prefix, path), 'rb')
                    return fd.read()
                finally:
                    if fd:
                        fd.close()

            getdata = getdatafn

        write(f, 'x' in ff and 0755 or 0644, 'l' in ff, getdata)

    for subpath in sorted(ctx.substate):
        sub = ctx.sub(subpath)
        submatch = match_.narrowmatcher(subpath, match)
        sub.archive(ui, archiver, os.path.join(prefix, repo._path) + '/',
                    submatch)

# If a largefile is modified, the change is not reflected in its
# standin until a commit. cmdutil.bailifchanged() raises an exception
# if the repo has uncommitted changes. Wrap it to also check if
# largefiles were changed. This is used by bisect and backout.
def overridebailifchanged(orig, repo):
    orig(repo)
    repo.lfstatus = True
    modified, added, removed, deleted = repo.status()[:4]
    repo.lfstatus = False
    if modified or added or removed or deleted:
        raise util.Abort(_('uncommitted changes'))

# Fetch doesn't use cmdutil.bailifchanged so override it to add the check
def overridefetch(orig, ui, repo, *pats, **opts):
    repo.lfstatus = True
    modified, added, removed, deleted = repo.status()[:4]
    repo.lfstatus = False
    if modified or added or removed or deleted:
        raise util.Abort(_('uncommitted changes'))
    return orig(ui, repo, *pats, **opts)

def overrideforget(orig, ui, repo, *pats, **opts):
    installnormalfilesmatchfn(repo[None].manifest())
    result = orig(ui, repo, *pats, **opts)
    restorematchfn()
    m = scmutil.match(repo[None], pats, opts)

    try:
        repo.lfstatus = True
        s = repo.status(match=m, clean=True)
    finally:
        repo.lfstatus = False
    forget = sorted(s[0] + s[1] + s[3] + s[6])
    forget = [f for f in forget if lfutil.standin(f) in repo[None].manifest()]

    for f in forget:
        if lfutil.standin(f) not in repo.dirstate and not \
                os.path.isdir(m.rel(lfutil.standin(f))):
            ui.warn(_('not removing %s: file is already untracked\n')
                    % m.rel(f))
            result = 1

    for f in forget:
        if ui.verbose or not m.exact(f):
            ui.status(_('removing %s\n') % m.rel(f))

    # Need to lock because standin files are deleted then removed from the
    # repository and we could race in-between.
    wlock = repo.wlock()
    try:
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        for f in forget:
            if lfdirstate[f] == 'a':
                lfdirstate.drop(f)
            else:
                lfdirstate.remove(f)
        lfdirstate.write()
        standins = [lfutil.standin(f) for f in forget]
        for f in standins:
            util.unlinkpath(repo.wjoin(f), ignoremissing=True)
        repo[None].forget(standins)
    finally:
        wlock.release()

    return result

def _getoutgoings(repo, other, missing, addfunc):
    """get pairs of filename and largefile hash in outgoing revisions
    in 'missing'.

    largefiles already existing on 'other' repository are ignored.

    'addfunc' is invoked with each unique pairs of filename and
    largefile hash value.
    """
    knowns = set()
    lfhashes = set()
    def dedup(fn, lfhash):
        k = (fn, lfhash)
        if k not in knowns:
            knowns.add(k)
            lfhashes.add(lfhash)
    lfutil.getlfilestoupload(repo, missing, dedup)
    if lfhashes:
        lfexists = basestore._openstore(repo, other).exists(lfhashes)
        for fn, lfhash in knowns:
            if not lfexists[lfhash]: # lfhash doesn't exist on "other"
                addfunc(fn, lfhash)

def outgoinghook(ui, repo, other, opts, missing):
    if opts.pop('large', None):
        lfhashes = set()
        if ui.debugflag:
            toupload = {}
            def addfunc(fn, lfhash):
                if fn not in toupload:
                    toupload[fn] = []
                toupload[fn].append(lfhash)
                lfhashes.add(lfhash)
            def showhashes(fn):
                for lfhash in sorted(toupload[fn]):
                    ui.debug('    %s\n' % (lfhash))
        else:
            toupload = set()
            def addfunc(fn, lfhash):
                toupload.add(fn)
                lfhashes.add(lfhash)
            def showhashes(fn):
                pass
        _getoutgoings(repo, other, missing, addfunc)

        if not toupload:
            ui.status(_('largefiles: no files to upload\n'))
        else:
            ui.status(_('largefiles to upload (%d entities):\n')
                      % (len(lfhashes)))
            for file in sorted(toupload):
                ui.status(lfutil.splitstandin(file) + '\n')
                showhashes(file)
            ui.status('\n')

def summaryremotehook(ui, repo, opts, changes):
    largeopt = opts.get('large', False)
    if changes is None:
        if largeopt:
            return (False, True) # only outgoing check is needed
        else:
            return (False, False)
    elif largeopt:
        url, branch, peer, outgoing = changes[1]
        if peer is None:
            # i18n: column positioning for "hg summary"
            ui.status(_('largefiles: (no remote repo)\n'))
            return

        toupload = set()
        lfhashes = set()
        def addfunc(fn, lfhash):
            toupload.add(fn)
            lfhashes.add(lfhash)
        _getoutgoings(repo, peer, outgoing.missing, addfunc)

        if not toupload:
            # i18n: column positioning for "hg summary"
            ui.status(_('largefiles: (no files to upload)\n'))
        else:
            # i18n: column positioning for "hg summary"
            ui.status(_('largefiles: %d entities for %d files to upload\n')
                      % (len(lfhashes), len(toupload)))

def overridesummary(orig, ui, repo, *pats, **opts):
    try:
        repo.lfstatus = True
        orig(ui, repo, *pats, **opts)
    finally:
        repo.lfstatus = False

def scmutiladdremove(orig, repo, pats=[], opts={}, dry_run=None,
                     similarity=None):
    if not lfutil.islfilesrepo(repo):
        return orig(repo, pats, opts, dry_run, similarity)
    # Get the list of missing largefiles so we can remove them
    lfdirstate = lfutil.openlfdirstate(repo.ui, repo)
    s = lfdirstate.status(match_.always(repo.root, repo.getcwd()), [], False,
        False, False)
    (unsure, modified, added, removed, missing, unknown, ignored, clean) = s

    # Call into the normal remove code, but the removing of the standin, we want
    # to have handled by original addremove.  Monkey patching here makes sure
    # we don't remove the standin in the largefiles code, preventing a very
    # confused state later.
    if missing:
        m = [repo.wjoin(f) for f in missing]
        repo._isaddremove = True
        removelargefiles(repo.ui, repo, *m, **opts)
        repo._isaddremove = False
    # Call into the normal add code, and any files that *should* be added as
    # largefiles will be
    addlargefiles(repo.ui, repo, *pats, **opts)
    # Now that we've handled largefiles, hand off to the original addremove
    # function to take care of the rest.  Make sure it doesn't do anything with
    # largefiles by installing a matcher that will ignore them.
    installnormalfilesmatchfn(repo[None].manifest())
    result = orig(repo, pats, opts, dry_run, similarity)
    restorematchfn()
    return result

# Calling purge with --all will cause the largefiles to be deleted.
# Override repo.status to prevent this from happening.
def overridepurge(orig, ui, repo, *dirs, **opts):
    # XXX large file status is buggy when used on repo proxy.
    # XXX this needs to be investigate.
    repo = repo.unfiltered()
    oldstatus = repo.status
    def overridestatus(node1='.', node2=None, match=None, ignored=False,
                        clean=False, unknown=False, listsubrepos=False):
        r = oldstatus(node1, node2, match, ignored, clean, unknown,
                      listsubrepos)
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        modified, added, removed, deleted, unknown, ignored, clean = r
        unknown = [f for f in unknown if lfdirstate[f] == '?']
        ignored = [f for f in ignored if lfdirstate[f] == '?']
        return modified, added, removed, deleted, unknown, ignored, clean
    repo.status = overridestatus
    orig(ui, repo, *dirs, **opts)
    repo.status = oldstatus

def overriderollback(orig, ui, repo, **opts):
    wlock = repo.wlock()
    try:
        before = repo.dirstate.parents()
        orphans = set(f for f in repo.dirstate
                      if lfutil.isstandin(f) and repo.dirstate[f] != 'r')
        result = orig(ui, repo, **opts)
        after = repo.dirstate.parents()
        if before == after:
            return result # no need to restore standins

        pctx = repo['.']
        for f in repo.dirstate:
            if lfutil.isstandin(f):
                orphans.discard(f)
                if repo.dirstate[f] == 'r':
                    repo.wvfs.unlinkpath(f, ignoremissing=True)
                elif f in pctx:
                    fctx = pctx[f]
                    repo.wwrite(f, fctx.data(), fctx.flags())
                else:
                    # content of standin is not so important in 'a',
                    # 'm' or 'n' (coming from the 2nd parent) cases
                    lfutil.writestandin(repo, f, '', False)
        for standin in orphans:
            repo.wvfs.unlinkpath(standin, ignoremissing=True)

        lfdirstate = lfutil.openlfdirstate(ui, repo)
        orphans = set(lfdirstate)
        lfiles = lfutil.listlfiles(repo)
        for file in lfiles:
            lfutil.synclfdirstate(repo, lfdirstate, file, True)
            orphans.discard(file)
        for lfile in orphans:
            lfdirstate.drop(lfile)
        lfdirstate.write()
    finally:
        wlock.release()
    return result

def overridetransplant(orig, ui, repo, *revs, **opts):
    try:
        oldstandins = lfutil.getstandinsstate(repo)
        repo._istransplanting = True
        result = orig(ui, repo, *revs, **opts)
        newstandins = lfutil.getstandinsstate(repo)
        filelist = lfutil.getlfilestoupdate(oldstandins, newstandins)
        lfcommands.updatelfiles(repo.ui, repo, filelist=filelist,
                                printmessage=True)
    finally:
        repo._istransplanting = False
    return result

def overridecat(orig, ui, repo, file1, *pats, **opts):
    ctx = scmutil.revsingle(repo, opts.get('rev'))
    err = 1
    notbad = set()
    m = scmutil.match(ctx, (file1,) + pats, opts)
    origmatchfn = m.matchfn
    def lfmatchfn(f):
        if origmatchfn(f):
            return True
        lf = lfutil.splitstandin(f)
        if lf is None:
            return False
        notbad.add(lf)
        return origmatchfn(lf)
    m.matchfn = lfmatchfn
    origbadfn = m.bad
    def lfbadfn(f, msg):
        if not f in notbad:
            origbadfn(f, msg)
    m.bad = lfbadfn
    for f in ctx.walk(m):
        fp = cmdutil.makefileobj(repo, opts.get('output'), ctx.node(),
                                 pathname=f)
        lf = lfutil.splitstandin(f)
        if lf is None or origmatchfn(f):
            # duplicating unreachable code from commands.cat
            data = ctx[f].data()
            if opts.get('decode'):
                data = repo.wwritedata(f, data)
            fp.write(data)
        else:
            hash = lfutil.readstandin(repo, lf, ctx.rev())
            if not lfutil.inusercache(repo.ui, hash):
                store = basestore._openstore(repo)
                success, missing = store.get([(lf, hash)])
                if len(success) != 1:
                    raise util.Abort(
                        _('largefile %s is not in cache and could not be '
                          'downloaded')  % lf)
            path = lfutil.usercachepath(repo.ui, hash)
            fpin = open(path, "rb")
            for chunk in util.filechunkiter(fpin, 128 * 1024):
                fp.write(chunk)
            fpin.close()
        fp.close()
        err = 0
    return err

def mercurialsinkbefore(orig, sink):
    sink.repo._isconverting = True
    orig(sink)

def mercurialsinkafter(orig, sink):
    sink.repo._isconverting = False
    orig(sink)

def mergeupdate(orig, repo, node, branchmerge, force, partial,
                *args, **kwargs):
    wlock = repo.wlock()
    try:
        # branch |       |         |
        #  merge | force | partial | action
        # -------+-------+---------+--------------
        #    x   |   x   |    x    | linear-merge
        #    o   |   x   |    x    | branch-merge
        #    x   |   o   |    x    | overwrite (as clean update)
        #    o   |   o   |    x    | force-branch-merge (*1)
        #    x   |   x   |    o    |   (*)
        #    o   |   x   |    o    |   (*)
        #    x   |   o   |    o    | overwrite (as revert)
        #    o   |   o   |    o    |   (*)
        #
        # (*) don't care
        # (*1) deprecated, but used internally (e.g: "rebase --collapse")

        linearmerge = not branchmerge and not force and not partial

        if linearmerge or (branchmerge and force and not partial):
            # update standins for linear-merge or force-branch-merge,
            # because largefiles in the working directory may be modified
            lfdirstate = lfutil.openlfdirstate(repo.ui, repo)
            s = lfdirstate.status(match_.always(repo.root, repo.getcwd()),
                                  [], False, False, False)
            unsure, modified, added = s[:3]
            for lfile in unsure + modified + added:
                lfutil.updatestandin(repo, lfutil.standin(lfile))

        if linearmerge:
            # Only call updatelfiles on the standins that have changed
            # to save time
            oldstandins = lfutil.getstandinsstate(repo)

        result = orig(repo, node, branchmerge, force, partial, *args, **kwargs)

        filelist = None
        if linearmerge:
            newstandins = lfutil.getstandinsstate(repo)
            filelist = lfutil.getlfilestoupdate(oldstandins, newstandins)

        # suppress status message while automated committing
        printmessage = not (getattr(repo, "_isrebasing", False) or
                            getattr(repo, "_istransplanting", False))
        lfcommands.updatelfiles(repo.ui, repo, filelist=filelist,
                                printmessage=printmessage,
                                normallookup=partial)

        return result
    finally:
        wlock.release()
