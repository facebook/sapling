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
    node, archival, error, merge
from mercurial.i18n import _
from mercurial.node import hex
from hgext import rebase

import lfutil
import lfcommands

# -- Utility functions: commonly/repeatedly needed functionality ---------------

def installnormalfilesmatchfn(manifest):
    '''overrides scmutil.match so that the matcher it returns will ignore all
    largefiles'''
    oldmatch = None # for the closure
    def override_match(ctx, pats=[], opts={}, globbed=False,
            default='relpath'):
        match = oldmatch(ctx, pats, opts, globbed, default)
        m = copy.copy(match)
        notlfile = lambda f: not (lfutil.isstandin(f) or lfutil.standin(f) in
                manifest)
        m._files = filter(notlfile, m._files)
        m._fmap = set(m._files)
        orig_matchfn = m.matchfn
        m.matchfn = lambda f: notlfile(f) and orig_matchfn(f) or None
        return m
    oldmatch = installmatchfn(override_match)

def installmatchfn(f):
    oldmatch = scmutil.match
    setattr(f, 'oldmatch', oldmatch)
    scmutil.match = f
    return oldmatch

def restorematchfn():
    '''restores scmutil.match to what it was before installnormalfilesmatchfn
    was called.  no-op if scmutil.match is its original function.

    Note that n calls to installnormalfilesmatchfn will require n calls to
    restore matchfn to reverse'''
    scmutil.match = getattr(scmutil.match, 'oldmatch', scmutil.match)

def add_largefiles(ui, repo, *pats, **opts):
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

        if exact or not exists:
            abovemin = (lfsize and
                        os.lstat(repo.wjoin(f)).st_size >= lfsize * 1024 * 1024)
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
                    for f in lfutil.repo_add(repo, standins)
                    if f in m.files()]
    finally:
        wlock.release()
    return bad

def remove_largefiles(ui, repo, *pats, **opts):
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

    def warn(files, reason):
        for f in files:
            ui.warn(_('not removing %s: %s (use forget to undo)\n')
                    % (m.rel(f), reason))

    if after:
        remove, forget = deleted, []
        warn(modified + added + clean, _('file still exists'))
    else:
        remove, forget = deleted + clean, []
        warn(modified, _('file is modified'))
        warn(added, _('file has been marked for add'))

    for f in sorted(remove + forget):
        if ui.verbose or not m.exact(f):
            ui.status(_('removing %s\n') % m.rel(f))

    # Need to lock because standin files are deleted then removed from the
    # repository and we could race inbetween.
    wlock = repo.wlock()
    try:
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        for f in remove:
            if not after:
                # If this is being called by addremove, notify the user that we
                # are removing the file.
                if getattr(repo, "_isaddremove", False):
                    ui.status(_('removing %s\n') % f)
                if os.path.exists(repo.wjoin(f)):
                    util.unlinkpath(repo.wjoin(f))
            lfdirstate.remove(f)
        lfdirstate.write()
        forget = [lfutil.standin(f) for f in forget]
        remove = [lfutil.standin(f) for f in remove]
        lfutil.repo_forget(repo, forget)
        # If this is being called by addremove, let the original addremove
        # function handle this.
        if not getattr(repo, "_isaddremove", False):
            lfutil.repo_remove(repo, remove, unlink=True)
    finally:
        wlock.release()

# -- Wrappers: modify existing commands --------------------------------

# Add works by going through the files that the user wanted to add and
# checking if they should be added as largefiles. Then it makes a new
# matcher which matches only the normal files and runs the original
# version of add.
def override_add(orig, ui, repo, *pats, **opts):
    normal = opts.pop('normal')
    if normal:
        if opts.get('large'):
            raise util.Abort(_('--normal cannot be used with --large'))
        return orig(ui, repo, *pats, **opts)
    bad = add_largefiles(ui, repo, *pats, **opts)
    installnormalfilesmatchfn(repo[None].manifest())
    result = orig(ui, repo, *pats, **opts)
    restorematchfn()

    return (result == 1 or bad) and 1 or 0

def override_remove(orig, ui, repo, *pats, **opts):
    installnormalfilesmatchfn(repo[None].manifest())
    orig(ui, repo, *pats, **opts)
    restorematchfn()
    remove_largefiles(ui, repo, *pats, **opts)

def override_status(orig, ui, repo, *pats, **opts):
    try:
        repo.lfstatus = True
        return orig(ui, repo, *pats, **opts)
    finally:
        repo.lfstatus = False

def override_log(orig, ui, repo, *pats, **opts):
    try:
        repo.lfstatus = True
        orig(ui, repo, *pats, **opts)
    finally:
        repo.lfstatus = False

def override_verify(orig, ui, repo, *pats, **opts):
    large = opts.pop('large', False)
    all = opts.pop('lfa', False)
    contents = opts.pop('lfc', False)

    result = orig(ui, repo, *pats, **opts)
    if large:
        result = result or lfcommands.verifylfiles(ui, repo, all, contents)
    return result

# Override needs to refresh standins so that update's normal merge
# will go through properly. Then the other update hook (overriding repo.update)
# will get the new files. Filemerge is also overriden so that the merge
# will merge standins correctly.
def override_update(orig, ui, repo, *pats, **opts):
    lfdirstate = lfutil.openlfdirstate(ui, repo)
    s = lfdirstate.status(match_.always(repo.root, repo.getcwd()), [], False,
        False, False)
    (unsure, modified, added, removed, missing, unknown, ignored, clean) = s

    # Need to lock between the standins getting updated and their
    # largefiles getting updated
    wlock = repo.wlock()
    try:
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
                raise util.Abort(_('uncommitted local changes'))
        # XXX handle removed differently
        if not opts['clean']:
            for lfile in unsure + modified + added:
                lfutil.updatestandin(repo, lfutil.standin(lfile))
    finally:
        wlock.release()
    return orig(ui, repo, *pats, **opts)

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
def override_checkunknown(origfn, wctx, mctx, folding):
    origunknown = wctx.unknown()
    wctx._unknown = filter(lambda f: lfutil.standin(f) not in wctx, origunknown)
    try:
        return origfn(wctx, mctx, folding)
    finally:
        wctx._unknown = origunknown

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
def override_manifestmerge(origfn, repo, p1, p2, pa, overwrite, partial):
    actions = origfn(repo, p1, p2, pa, overwrite, partial)
    processed = []

    for action in actions:
        if overwrite:
            processed.append(action)
            continue
        f, m = action[:2]

        choices = (_('&Largefile'), _('&Normal file'))
        if m == "g" and lfutil.splitstandin(f) in p1 and f in p2:
            # Case 1: normal file in the working copy, largefile in
            # the second parent
            lfile = lfutil.splitstandin(f)
            standin = f
            msg = _('%s has been turned into a largefile\n'
                    'use (l)argefile or keep as (n)ormal file?') % lfile
            if repo.ui.promptchoice(msg, choices, 0) == 0:
                processed.append((lfile, "r"))
                processed.append((standin, "g", p2.flags(standin)))
            else:
                processed.append((standin, "r"))
        elif m == "m" and lfutil.standin(f) in p1 and f in p2:
            # Case 2: largefile in the working copy, normal file in
            # the second parent
            standin = lfutil.standin(f)
            lfile = f
            msg = _('%s has been turned into a normal file\n'
                    'keep as (l)argefile or use (n)ormal file?') % lfile
            if repo.ui.promptchoice(msg, choices, 0) == 0:
                processed.append((lfile, "r"))
            else:
                processed.append((standin, "r"))
                processed.append((lfile, "g", p2.flags(lfile)))
        else:
            processed.append(action)

    return processed

# Override filemerge to prompt the user about how they wish to merge
# largefiles. This will handle identical edits, and copy/rename +
# edit without prompting the user.
def override_filemerge(origfn, repo, mynode, orig, fcd, fco, fca):
    # Use better variable names here. Because this is a wrapper we cannot
    # change the variable names in the function declaration.
    fcdest, fcother, fcancestor = fcd, fco, fca
    if not lfutil.isstandin(orig):
        return origfn(repo, mynode, orig, fcdest, fcother, fcancestor)
    else:
        if not fcother.cmp(fcdest): # files identical?
            return None

        # backwards, use working dir parent as ancestor
        if fcancestor == fcother:
            fcancestor = fcdest.parents()[0]

        if orig != fcother.path():
            repo.ui.status(_('merging %s and %s to %s\n')
                           % (lfutil.splitstandin(orig),
                              lfutil.splitstandin(fcother.path()),
                              lfutil.splitstandin(fcdest.path())))
        else:
            repo.ui.status(_('merging %s\n')
                           % lfutil.splitstandin(fcdest.path()))

        if fcancestor.path() != fcother.path() and fcother.data() == \
                fcancestor.data():
            return 0
        if fcancestor.path() != fcdest.path() and fcdest.data() == \
                fcancestor.data():
            repo.wwrite(fcdest.path(), fcother.data(), fcother.flags())
            return 0

        if repo.ui.promptchoice(_('largefile %s has a merge conflict\n'
                             'keep (l)ocal or take (o)ther?') %
                             lfutil.splitstandin(orig),
                             (_('&Local'), _('&Other')), 0) == 0:
            return 0
        else:
            repo.wwrite(fcdest.path(), fcother.data(), fcother.flags())
            return 0

# Copy first changes the matchers to match standins instead of
# largefiles.  Then it overrides util.copyfile in that function it
# checks if the destination largefile already exists. It also keeps a
# list of copied files so that the largefiles can be copied and the
# dirstate updated.
def override_copy(orig, ui, repo, pats, opts, rename=False):
    # doesn't remove largefile on rename
    if len(pats) < 2:
        # this isn't legal, let the original function deal with it
        return orig(ui, repo, pats, opts, rename)

    def makestandin(relpath):
        path = scmutil.canonpath(repo.root, repo.getcwd(), relpath)
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
    try:
        try:
            installnormalfilesmatchfn(repo[None].manifest())
            result = orig(ui, repo, pats, opts, rename)
        except util.Abort, e:
            if str(e) != 'no files to copy':
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
            # When we call orig below it creates the standins but we don't add them
            # to the dir state until later so lock during that time.
            wlock = repo.wlock()

            manifest = repo[None].manifest()
            oldmatch = None # for the closure
            def override_match(ctx, pats=[], opts={}, globbed=False,
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
                orig_matchfn = m.matchfn
                m.matchfn = lambda f: (lfutil.isstandin(f) and
                                    lfile(lfutil.splitstandin(f)) and
                                    orig_matchfn(lfutil.splitstandin(f)) or
                                    None)
                return m
            oldmatch = installmatchfn(override_match)
            listpats = []
            for pat in pats:
                if match_.patkind(pat) is not None:
                    listpats.append(pat)
                else:
                    listpats.append(makestandin(pat))

            try:
                origcopyfile = util.copyfile
                copiedfiles = []
                def override_copyfile(src, dest):
                    if (lfutil.shortname in src and
                        dest.startswith(repo.wjoin(lfutil.shortname))):
                        destlfile = dest.replace(lfutil.shortname, '')
                        if not opts['force'] and os.path.exists(destlfile):
                            raise IOError('',
                                _('destination largefile already exists'))
                    copiedfiles.append((src, dest))
                    origcopyfile(src, dest)

                util.copyfile = override_copyfile
                result += orig(ui, repo, listpats, opts, rename)
            finally:
                util.copyfile = origcopyfile

            lfdirstate = lfutil.openlfdirstate(ui, repo)
            for (src, dest) in copiedfiles:
                if (lfutil.shortname in src and
                    dest.startswith(repo.wjoin(lfutil.shortname))):
                    srclfile = src.replace(repo.wjoin(lfutil.standin('')), '')
                    destlfile = dest.replace(repo.wjoin(lfutil.standin('')), '')
                    destlfiledir = os.path.dirname(destlfile) or '.'
                    if not os.path.isdir(destlfiledir):
                        os.makedirs(destlfiledir)
                    if rename:
                        os.rename(repo.wjoin(srclfile), repo.wjoin(destlfile))
                        lfdirstate.remove(srclfile)
                    else:
                        util.copyfile(srclfile, destlfile)
                    lfdirstate.add(destlfile)
            lfdirstate.write()
        except util.Abort, e:
            if str(e) != 'no files to copy':
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
# resulting standins update the largefiles. Then return the standins
# to their proper state
def override_revert(orig, ui, repo, *pats, **opts):
    # Because we put the standins in a bad state (by updating them)
    # and then return them to a correct state we need to lock to
    # prevent others from changing them in their incorrect state.
    wlock = repo.wlock()
    try:
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        (modified, added, removed, missing, unknown, ignored, clean) = \
            lfutil.lfdirstate_status(lfdirstate, repo, repo['.'].rev())
        for lfile in modified:
            lfutil.updatestandin(repo, lfutil.standin(lfile))
        for lfile in missing:
            os.unlink(repo.wjoin(lfutil.standin(lfile)))

        try:
            ctx = repo[opts.get('rev')]
            oldmatch = None # for the closure
            def override_match(ctx, pats=[], opts={}, globbed=False,
                    default='relpath'):
                match = oldmatch(ctx, pats, opts, globbed, default)
                m = copy.copy(match)
                def tostandin(f):
                    if lfutil.standin(f) in ctx or lfutil.standin(f) in ctx:
                        return lfutil.standin(f)
                    elif lfutil.standin(f) in repo[None]:
                        return None
                    return f
                m._files = [tostandin(f) for f in m._files]
                m._files = [f for f in m._files if f is not None]
                m._fmap = set(m._files)
                orig_matchfn = m.matchfn
                def matchfn(f):
                    if lfutil.isstandin(f):
                        # We need to keep track of what largefiles are being
                        # matched so we know which ones to update later --
                        # otherwise we accidentally revert changes to other
                        # largefiles. This is repo-specific, so duckpunch the
                        # repo object to keep the list of largefiles for us
                        # later.
                        if orig_matchfn(lfutil.splitstandin(f)) and \
                                (f in repo[None] or f in ctx):
                            lfileslist = getattr(repo, '_lfilestoupdate', [])
                            lfileslist.append(lfutil.splitstandin(f))
                            repo._lfilestoupdate = lfileslist
                            return True
                        else:
                            return False
                    return orig_matchfn(f)
                m.matchfn = matchfn
                return m
            oldmatch = installmatchfn(override_match)
            scmutil.match
            matches = override_match(repo[None], pats, opts)
            orig(ui, repo, *pats, **opts)
        finally:
            restorematchfn()
        lfileslist = getattr(repo, '_lfilestoupdate', [])
        lfcommands.updatelfiles(ui, repo, filelist=lfileslist,
                                printmessage=False)

        # empty out the largefiles list so we start fresh next time
        repo._lfilestoupdate = []
        for lfile in modified:
            if lfile in lfileslist:
                if os.path.exists(repo.wjoin(lfutil.standin(lfile))) and lfile\
                        in repo['.']:
                    lfutil.writestandin(repo, lfutil.standin(lfile),
                        repo['.'][lfile].data().strip(),
                        'x' in repo['.'][lfile].flags())
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        for lfile in added:
            standin = lfutil.standin(lfile)
            if standin not in ctx and (standin in matches or opts.get('all')):
                if lfile in lfdirstate:
                    lfdirstate.drop(lfile)
                util.unlinkpath(repo.wjoin(standin))
        lfdirstate.write()
    finally:
        wlock.release()

def hg_update(orig, repo, node):
    result = orig(repo, node)
    lfcommands.updatelfiles(repo.ui, repo)
    return result

def hg_clean(orig, repo, node, show_stats=True):
    result = orig(repo, node, show_stats)
    lfcommands.updatelfiles(repo.ui, repo)
    return result

def hg_merge(orig, repo, node, force=None, remind=True):
    # Mark the repo as being in the middle of a merge, so that
    # updatelfiles() will know that it needs to trust the standins in
    # the working copy, not in the standins in the current node
    repo._ismerging = True
    try:
        result = orig(repo, node, force, remind)
        lfcommands.updatelfiles(repo.ui, repo)
    finally:
        repo._ismerging = False
    return result

# When we rebase a repository with remotely changed largefiles, we need to
# take some extra care so that the largefiles are correctly updated in the
# working copy
def override_pull(orig, ui, repo, source=None, **opts):
    if opts.get('rebase', False):
        repo._isrebasing = True
        try:
            if opts.get('update'):
                 del opts['update']
                 ui.debug('--update and --rebase are not compatible, ignoring '
                          'the update flag\n')
            del opts['rebase']
            cmdutil.bailifchanged(repo)
            revsprepull = len(repo)
            origpostincoming = commands.postincoming
            def _dummy(*args, **kwargs):
                pass
            commands.postincoming = _dummy
            repo.lfpullsource = source
            if not source:
                source = 'default'
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
        repo.lfpullsource = source
        if not source:
            source = 'default'
        oldheads = lfutil.getcurrentheads(repo)
        result = orig(ui, repo, source, **opts)
        # If we do not have the new largefiles for any new heads we pulled, we
        # will run into a problem later if we try to merge or rebase with one of
        # these heads, so cache the largefiles now direclty into the system
        # cache.
        ui.status(_("caching new largefiles\n"))
        numcached = 0
        heads = lfutil.getcurrentheads(repo)
        newheads = set(heads).difference(set(oldheads))
        for head in newheads:
            (cached, missing) = lfcommands.cachelfiles(ui, repo, head)
            numcached += len(cached)
        ui.status(_("%d largefiles cached\n") % numcached)
    return result

def override_rebase(orig, ui, repo, **opts):
    repo._isrebasing = True
    try:
        orig(ui, repo, **opts)
    finally:
        repo._isrebasing = False

def override_archive(orig, repo, dest, node, kind, decode=True, matchfn=None,
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
        for subpath in ctx.substate:
            sub = ctx.sub(subpath)
            sub.archive(repo.ui, archiver, prefix)

    archiver.done()

# If a largefile is modified, the change is not reflected in its
# standin until a commit. cmdutil.bailifchanged() raises an exception
# if the repo has uncommitted changes. Wrap it to also check if
# largefiles were changed. This is used by bisect and backout.
def override_bailifchanged(orig, repo):
    orig(repo)
    repo.lfstatus = True
    modified, added, removed, deleted = repo.status()[:4]
    repo.lfstatus = False
    if modified or added or removed or deleted:
        raise util.Abort(_('outstanding uncommitted changes'))

# Fetch doesn't use cmdutil.bail_if_changed so override it to add the check
def override_fetch(orig, ui, repo, *pats, **opts):
    repo.lfstatus = True
    modified, added, removed, deleted = repo.status()[:4]
    repo.lfstatus = False
    if modified or added or removed or deleted:
        raise util.Abort(_('outstanding uncommitted changes'))
    return orig(ui, repo, *pats, **opts)

def override_forget(orig, ui, repo, *pats, **opts):
    installnormalfilesmatchfn(repo[None].manifest())
    orig(ui, repo, *pats, **opts)
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

    for f in forget:
        if ui.verbose or not m.exact(f):
            ui.status(_('removing %s\n') % m.rel(f))

    # Need to lock because standin files are deleted then removed from the
    # repository and we could race inbetween.
    wlock = repo.wlock()
    try:
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        for f in forget:
            if lfdirstate[f] == 'a':
                lfdirstate.drop(f)
            else:
                lfdirstate.remove(f)
        lfdirstate.write()
        lfutil.repo_remove(repo, [lfutil.standin(f) for f in forget],
            unlink=True)
    finally:
        wlock.release()

def getoutgoinglfiles(ui, repo, dest=None, **opts):
    dest = ui.expandpath(dest or 'default-push', dest or 'default')
    dest, branches = hg.parseurl(dest, opts.get('branch'))
    revs, checkout = hg.addbranchrevs(repo, repo, branches, opts.get('rev'))
    if revs:
        revs = [repo.lookup(rev) for rev in revs]

    remoteui = hg.remoteui

    try:
        remote = hg.repository(remoteui(repo, opts), dest)
    except error.RepoError:
        return None
    o = lfutil.findoutgoing(repo, remote, False)
    if not o:
        return None
    o = repo.changelog.nodesbetween(o, revs)[0]
    if opts.get('newest_first'):
        o.reverse()

    toupload = set()
    for n in o:
        parents = [p for p in repo.changelog.parents(n) if p != node.nullid]
        ctx = repo[n]
        files = set(ctx.files())
        if len(parents) == 2:
            mc = ctx.manifest()
            mp1 = ctx.parents()[0].manifest()
            mp2 = ctx.parents()[1].manifest()
            for f in mp1:
                if f not in mc:
                        files.add(f)
            for f in mp2:
                if f not in mc:
                    files.add(f)
            for f in mc:
                if mc[f] != mp1.get(f, None) or mc[f] != mp2.get(f, None):
                    files.add(f)
        toupload = toupload.union(
            set([f for f in files if lfutil.isstandin(f) and f in ctx]))
    return toupload

def override_outgoing(orig, ui, repo, dest=None, **opts):
    orig(ui, repo, dest, **opts)

    if opts.pop('large', None):
        toupload = getoutgoinglfiles(ui, repo, dest, **opts)
        if toupload is None:
            ui.status(_('largefiles: No remote repo\n'))
        else:
            ui.status(_('largefiles to upload:\n'))
            for file in toupload:
                ui.status(lfutil.splitstandin(file) + '\n')
            ui.status('\n')

def override_summary(orig, ui, repo, *pats, **opts):
    try:
        repo.lfstatus = True
        orig(ui, repo, *pats, **opts)
    finally:
        repo.lfstatus = False

    if opts.pop('large', None):
        toupload = getoutgoinglfiles(ui, repo, None, **opts)
        if toupload is None:
            ui.status(_('largefiles: No remote repo\n'))
        else:
            ui.status(_('largefiles: %d to upload\n') % len(toupload))

def override_addremove(orig, ui, repo, *pats, **opts):
    # Get the list of missing largefiles so we can remove them
    lfdirstate = lfutil.openlfdirstate(ui, repo)
    s = lfdirstate.status(match_.always(repo.root, repo.getcwd()), [], False,
        False, False)
    (unsure, modified, added, removed, missing, unknown, ignored, clean) = s

    # Call into the normal remove code, but the removing of the standin, we want
    # to have handled by original addremove.  Monkey patching here makes sure
    # we don't remove the standin in the largefiles code, preventing a very
    # confused state later.
    if missing:
        repo._isaddremove = True
        remove_largefiles(ui, repo, *missing, **opts)
        repo._isaddremove = False
    # Call into the normal add code, and any files that *should* be added as
    # largefiles will be
    add_largefiles(ui, repo, *pats, **opts)
    # Now that we've handled largefiles, hand off to the original addremove
    # function to take care of the rest.  Make sure it doesn't do anything with
    # largefiles by installing a matcher that will ignore them.
    installnormalfilesmatchfn(repo[None].manifest())
    result = orig(ui, repo, *pats, **opts)
    restorematchfn()
    return result

# Calling purge with --all will cause the largefiles to be deleted.
# Override repo.status to prevent this from happening.
def override_purge(orig, ui, repo, *dirs, **opts):
    oldstatus = repo.status
    def override_status(node1='.', node2=None, match=None, ignored=False,
                        clean=False, unknown=False, listsubrepos=False):
        r = oldstatus(node1, node2, match, ignored, clean, unknown,
                      listsubrepos)
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        modified, added, removed, deleted, unknown, ignored, clean = r
        unknown = [f for f in unknown if lfdirstate[f] == '?']
        ignored = [f for f in ignored if lfdirstate[f] == '?']
        return modified, added, removed, deleted, unknown, ignored, clean
    repo.status = override_status
    orig(ui, repo, *dirs, **opts)
    repo.status = oldstatus

def override_rollback(orig, ui, repo, **opts):
    result = orig(ui, repo, **opts)
    merge.update(repo, node=None, branchmerge=False, force=True,
        partial=lfutil.isstandin)
    wlock = repo.wlock()
    try:
        lfdirstate = lfutil.openlfdirstate(ui, repo)
        lfiles = lfutil.listlfiles(repo)
        oldlfiles = lfutil.listlfiles(repo, repo[None].parents()[0].rev())
        for file in lfiles:
            if file in oldlfiles:
                lfdirstate.normallookup(file)
            else:
                lfdirstate.add(file)
        lfdirstate.write()
    finally:
        wlock.release()
    return result

def override_transplant(orig, ui, repo, *revs, **opts):
    try:
        repo._istransplanting = True
        result = orig(ui, repo, *revs, **opts)
        lfcommands.updatelfiles(ui, repo, filelist=None,
                                printmessage=False)
    finally:
        repo._istransplanting = False
    return result
