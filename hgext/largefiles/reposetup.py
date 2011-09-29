# Copyright 2009-2010 Gregory P. Ward
# Copyright 2009-2010 Intelerad Medical Systems Incorporated
# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''setup for largefiles repositories: reposetup'''
import copy
import types
import os
import re

from mercurial import context, error, manifest, match as match_, \
        node, util
from mercurial.i18n import _

import lfcommands
import proto
import lfutil

def reposetup(ui, repo):
    # wire repositories should be given new wireproto functions but not the
    # other largefiles modifications
    if not repo.local():
        return proto.wirereposetup(ui, repo)

    for name in ('status', 'commitctx', 'commit', 'push'):
        method = getattr(repo, name)
        #if not (isinstance(method, types.MethodType) and
        #        method.im_func is repo.__class__.commitctx.im_func):
        if isinstance(method, types.FunctionType) and method.func_name == \
            'wrap':
            ui.warn(_('largefiles: repo method %r appears to have already been'
                    ' wrapped by another extension: '
                    'largefiles may behave incorrectly\n')
                    % name)

    class lfiles_repo(repo.__class__):
        lfstatus = False
        def status_nolfiles(self, *args, **kwargs):
            return super(lfiles_repo, self).status(*args, **kwargs)

        # When lfstatus is set, return a context that gives the names of lfiles
        # instead of their corresponding standins and identifies the lfiles as
        # always binary, regardless of their actual contents.
        def __getitem__(self, changeid):
            ctx = super(lfiles_repo, self).__getitem__(changeid)
            if self.lfstatus:
                class lfiles_manifestdict(manifest.manifestdict):
                    def __contains__(self, filename):
                        if super(lfiles_manifestdict,
                                self).__contains__(filename):
                            return True
                        return super(lfiles_manifestdict,
                            self).__contains__(lfutil.shortname+'/' + filename)
                class lfiles_ctx(ctx.__class__):
                    def files(self):
                        filenames = super(lfiles_ctx, self).files()
                        return [re.sub('^\\'+lfutil.shortname+'/', '',
                                       filename) for filename in filenames]
                    def manifest(self):
                        man1 = super(lfiles_ctx, self).manifest()
                        man1.__class__ = lfiles_manifestdict
                        return man1
                    def filectx(self, path, fileid=None, filelog=None):
                        try:
                            result = super(lfiles_ctx, self).filectx(path,
                                fileid, filelog)
                        except error.LookupError:
                            # Adding a null character will cause Mercurial to
                            # identify this as a binary file.
                            result = super(lfiles_ctx, self).filectx(
                                lfutil.shortname + '/' + path, fileid,
                                filelog)
                            olddata = result.data
                            result.data = lambda: olddata() + '\0'
                        return result
                ctx.__class__ = lfiles_ctx
            return ctx

        # Figure out the status of big files and insert them into the
        # appropriate list in the result. Also removes standin files from
        # the listing. This function reverts to the original status if
        # self.lfstatus is False
        def status(self, node1='.', node2=None, match=None, ignored=False,
                clean=False, unknown=False, listsubrepos=False):
            listignored, listclean, listunknown = ignored, clean, unknown
            if not self.lfstatus:
                try:
                    return super(lfiles_repo, self).status(node1, node2, match,
                        listignored, listclean, listunknown, listsubrepos)
                except TypeError:
                    return super(lfiles_repo, self).status(node1, node2, match,
                        listignored, listclean, listunknown)
            else:
                # some calls in this function rely on the old version of status
                self.lfstatus = False
                if isinstance(node1, context.changectx):
                    ctx1 = node1
                else:
                    ctx1 = repo[node1]
                if isinstance(node2, context.changectx):
                    ctx2 = node2
                else:
                    ctx2 = repo[node2]
                working = ctx2.rev() is None
                parentworking = working and ctx1 == self['.']

                def inctx(file, ctx):
                    try:
                        if ctx.rev() is None:
                            return file in ctx.manifest()
                        ctx[file]
                        return True
                    except KeyError:
                        return False

                # create a copy of match that matches standins instead of
                # lfiles if matcher not set then it is the always matcher so
                # overwrite that
                if match is None:
                    match = match_.always(self.root, self.getcwd())

                def tostandin(file):
                    if inctx(lfutil.standin(file), ctx2):
                        return lfutil.standin(file)
                    return file

                m = copy.copy(match)
                m._files = [tostandin(f) for f in m._files]

                # get ignored clean and unknown but remove them later if they
                # were not asked for
                try:
                    result = super(lfiles_repo, self).status(node1, node2, m,
                        True, True, True, listsubrepos)
                except TypeError:
                    result = super(lfiles_repo, self).status(node1, node2, m,
                        True, True, True)
                if working:
                    # Hold the wlock while we read lfiles and update the
                    # lfdirstate
                    wlock = repo.wlock()
                    try:
                        # Any non lfiles that were explicitly listed must be
                        # taken out or lfdirstate.status will report an error.
                        # The status of these files was already computed using
                        # super's status.
                        lfdirstate = lfutil.openlfdirstate(ui, self)
                        match._files = [f for f in match._files if f in
                            lfdirstate]
                        s = lfdirstate.status(match, [], listignored,
                                listclean, listunknown)
                        (unsure, modified, added, removed, missing, unknown,
                                ignored, clean) = s
                        if parentworking:
                            for lfile in unsure:
                                if ctx1[lfutil.standin(lfile)].data().strip() \
                                        != lfutil.hashfile(self.wjoin(lfile)):
                                    modified.append(lfile)
                                else:
                                    clean.append(lfile)
                                    lfdirstate.normal(lfile)
                            lfdirstate.write()
                        else:
                            tocheck = unsure + modified + added + clean
                            modified, added, clean = [], [], []

                            for lfile in tocheck:
                                standin = lfutil.standin(lfile)
                                if inctx(standin, ctx1):
                                    if ctx1[standin].data().strip() != \
                                            lfutil.hashfile(self.wjoin(lfile)):
                                        modified.append(lfile)
                                    else:
                                        clean.append(lfile)
                                else:
                                    added.append(lfile)
                    finally:
                        wlock.release()

                    for standin in ctx1.manifest():
                        if not lfutil.isstandin(standin):
                            continue
                        lfile = lfutil.splitstandin(standin)
                        if not match(lfile):
                            continue
                        if lfile not in lfdirstate:
                            removed.append(lfile)
                    # Handle unknown and ignored differently
                    lfiles = (modified, added, removed, missing, [], [], clean)
                    result = list(result)
                    # Unknown files
                    result[4] = [f for f in unknown if repo.dirstate[f] == '?'\
                        and not lfutil.isstandin(f)]
                    # Ignored files must be ignored by both the dirstate and
                    # lfdirstate
                    result[5] = set(ignored).intersection(set(result[5]))
                    # combine normal files and lfiles
                    normals = [[fn for fn in filelist if not \
                        lfutil.isstandin(fn)] for filelist in result]
                    result = [sorted(list1 + list2) for (list1, list2) in \
                        zip(normals, lfiles)]
                else:
                    def toname(f):
                        if lfutil.isstandin(f):
                            return lfutil.splitstandin(f)
                        return f
                    result = [[toname(f) for f in items] for items in result]

                if not listunknown:
                    result[4] = []
                if not listignored:
                    result[5] = []
                if not listclean:
                    result[6] = []
                self.lfstatus = True
                return result

        # This call happens after a commit has occurred. Copy all of the lfiles
        # into the cache
        def commitctx(self, *args, **kwargs):
            node = super(lfiles_repo, self).commitctx(*args, **kwargs)
            ctx = self[node]
            for filename in ctx.files():
                if lfutil.isstandin(filename) and filename in ctx.manifest():
                    realfile = lfutil.splitstandin(filename)
                    lfutil.copytocache(self, ctx.node(), realfile)

            return node

        # This call happens before a commit has occurred. The lfile standins
        # have not had their contents updated (to reflect the hash of their
        # lfile).  Do that here.
        def commit(self, text="", user=None, date=None, match=None,
                force=False, editor=False, extra={}):
            orig = super(lfiles_repo, self).commit

            wlock = repo.wlock()
            try:
                if getattr(repo, "_isrebasing", False):
                    # We have to take the time to pull down the new lfiles now.
                    # Otherwise if we are rebasing, any lfiles that were
                    # modified in the changesets we are rebasing on top of get
                    # overwritten either by the rebase or in the first commit
                    # after the rebase.
                    lfcommands.updatelfiles(repo.ui, repo)
                # Case 1: user calls commit with no specific files or
                # include/exclude patterns: refresh and commit everything.
                if (match is None) or (not match.anypats() and not \
                        match.files()):
                    lfiles = lfutil.listlfiles(self)
                    lfdirstate = lfutil.openlfdirstate(ui, self)
                    # this only loops through lfiles that exist (not
                    # removed/renamed)
                    for lfile in lfiles:
                        if os.path.exists(self.wjoin(lfutil.standin(lfile))):
                            # this handles the case where a rebase is being
                            # performed and the working copy is not updated
                            # yet.
                            if os.path.exists(self.wjoin(lfile)):
                                lfutil.updatestandin(self,
                                    lfutil.standin(lfile))
                                lfdirstate.normal(lfile)
                    for lfile in lfdirstate:
                        if not os.path.exists(
                                repo.wjoin(lfutil.standin(lfile))):
                            try:
                                # Mercurial >= 1.9
                                lfdirstate.drop(lfile)
                            except AttributeError:
                                # Mercurial <= 1.8
                                lfdirstate.forget(lfile)
                    lfdirstate.write()

                    return orig(text=text, user=user, date=date, match=match,
                                    force=force, editor=editor, extra=extra)

                for file in match.files():
                    if lfutil.isstandin(file):
                        raise util.Abort(
                            "Don't commit largefile standin. Commit largefile.")

                # Case 2: user calls commit with specified patterns: refresh
                # any matching big files.
                smatcher = lfutil.composestandinmatcher(self, match)
                standins = lfutil.dirstate_walk(self.dirstate, smatcher)

                # No matching big files: get out of the way and pass control to
                # the usual commit() method.
                if not standins:
                    return orig(text=text, user=user, date=date, match=match,
                                    force=force, editor=editor, extra=extra)

                # Refresh all matching big files.  It's possible that the
                # commit will end up failing, in which case the big files will
                # stay refreshed.  No harm done: the user modified them and
                # asked to commit them, so sooner or later we're going to
                # refresh the standins.  Might as well leave them refreshed.
                lfdirstate = lfutil.openlfdirstate(ui, self)
                for standin in standins:
                    lfile = lfutil.splitstandin(standin)
                    if lfdirstate[lfile] <> 'r':
                        lfutil.updatestandin(self, standin)
                        lfdirstate.normal(lfile)
                    else:
                        try:
                            # Mercurial >= 1.9
                            lfdirstate.drop(lfile)
                        except AttributeError:
                            # Mercurial <= 1.8
                            lfdirstate.forget(lfile)
                lfdirstate.write()

                # Cook up a new matcher that only matches regular files or
                # standins corresponding to the big files requested by the
                # user.  Have to modify _files to prevent commit() from
                # complaining "not tracked" for big files.
                lfiles = lfutil.listlfiles(repo)
                match = copy.copy(match)
                orig_matchfn = match.matchfn

                # Check both the list of lfiles and the list of standins
                # because if a lfile was removed, it won't be in the list of
                # lfiles at this point
                match._files += sorted(standins)

                actualfiles = []
                for f in match._files:
                    fstandin = lfutil.standin(f)

                    # Ignore known lfiles and standins
                    if f in lfiles or fstandin in standins:
                        continue

                    # Append directory separator to avoid collisions
                    if not fstandin.endswith(os.sep):
                        fstandin += os.sep

                    # Prevalidate matching standin directories
                    if lfutil.any_(st for st in match._files if \
                            st.startswith(fstandin)):
                        continue
                    actualfiles.append(f)
                match._files = actualfiles

                def matchfn(f):
                    if orig_matchfn(f):
                        return f not in lfiles
                    else:
                        return f in standins

                match.matchfn = matchfn
                return orig(text=text, user=user, date=date, match=match,
                                force=force, editor=editor, extra=extra)
            finally:
                wlock.release()

        def push(self, remote, force=False, revs=None, newbranch=False):
            o = lfutil.findoutgoing(repo, remote, force)
            if o:
                toupload = set()
                o = repo.changelog.nodesbetween(o, revs)[0]
                for n in o:
                    parents = [p for p in repo.changelog.parents(n) if p != \
                        node.nullid]
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
                            if mc[f] != mp1.get(f, None) or mc[f] != mp2.get(f,
                                    None):
                                files.add(f)

                    toupload = toupload.union(set([ctx[f].data().strip() for f\
                        in files if lfutil.isstandin(f) and f in ctx]))
                lfcommands.uploadlfiles(ui, self, remote, toupload)
            # Mercurial >= 1.6 takes the newbranch argument, try that first.
            try:
                return super(lfiles_repo, self).push(remote, force, revs,
                    newbranch)
            except TypeError:
                return super(lfiles_repo, self).push(remote, force, revs)

    repo.__class__ = lfiles_repo

    def checkrequireslfiles(ui, repo, **kwargs):
        if 'largefiles' not in repo.requirements and lfutil.any_(
                lfutil.shortname+'/' in f[0] for f in repo.store.datafiles()):
            # work around bug in mercurial 1.9 whereby requirements is a list
            # on newly-cloned repos
            repo.requirements = set(repo.requirements)

            repo.requirements |= set(['largefiles'])
            repo._writerequirements()

    checkrequireslfiles(ui, repo)

    ui.setconfig('hooks', 'changegroup.lfiles', checkrequireslfiles)
    ui.setconfig('hooks', 'commit.lfiles', checkrequireslfiles)
