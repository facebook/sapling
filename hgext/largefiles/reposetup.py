# Copyright 2009-2010 Gregory P. Ward
# Copyright 2009-2010 Intelerad Medical Systems Incorporated
# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''setup for largefiles repositories: reposetup'''
import copy
import os

from mercurial import error, match as match_, util
from mercurial.i18n import _
from mercurial import scmutil, localrepo

import lfcommands
import lfutil

def reposetup(ui, repo):
    # wire repositories should be given new wireproto functions
    # by "proto.wirereposetup()" via "hg.wirepeersetupfuncs"
    if not repo.local():
        return

    class lfilesrepo(repo.__class__):
        # the mark to examine whether "repo" object enables largefiles or not
        _largefilesenabled = True

        lfstatus = False
        def status_nolfiles(self, *args, **kwargs):
            return super(lfilesrepo, self).status(*args, **kwargs)

        # When lfstatus is set, return a context that gives the names
        # of largefiles instead of their corresponding standins and
        # identifies the largefiles as always binary, regardless of
        # their actual contents.
        def __getitem__(self, changeid):
            ctx = super(lfilesrepo, self).__getitem__(changeid)
            if self.lfstatus:
                class lfilesctx(ctx.__class__):
                    def files(self):
                        filenames = super(lfilesctx, self).files()
                        return [lfutil.splitstandin(f) or f for f in filenames]
                    def manifest(self):
                        man1 = super(lfilesctx, self).manifest()
                        class lfilesmanifest(man1.__class__):
                            def __contains__(self, filename):
                                orig = super(lfilesmanifest, self).__contains__
                                return (orig(filename) or
                                        orig(lfutil.standin(filename)))
                        man1.__class__ = lfilesmanifest
                        return man1
                    def filectx(self, path, fileid=None, filelog=None):
                        orig = super(lfilesctx, self).filectx
                        try:
                            if filelog is not None:
                                result = orig(path, fileid, filelog)
                            else:
                                result = orig(path, fileid)
                        except error.LookupError:
                            # Adding a null character will cause Mercurial to
                            # identify this as a binary file.
                            if filelog is not None:
                                result = orig(lfutil.standin(path), fileid,
                                              filelog)
                            else:
                                result = orig(lfutil.standin(path), fileid)
                            olddata = result.data
                            result.data = lambda: olddata() + '\0'
                        return result
                ctx.__class__ = lfilesctx
            return ctx

        # Figure out the status of big files and insert them into the
        # appropriate list in the result. Also removes standin files
        # from the listing. Revert to the original status if
        # self.lfstatus is False.
        # XXX large file status is buggy when used on repo proxy.
        # XXX this needs to be investigated.
        @localrepo.unfilteredmethod
        def status(self, node1='.', node2=None, match=None, ignored=False,
                clean=False, unknown=False, listsubrepos=False):
            listignored, listclean, listunknown = ignored, clean, unknown
            orig = super(lfilesrepo, self).status
            if not self.lfstatus:
                return orig(node1, node2, match, listignored, listclean,
                            listunknown, listsubrepos)

            # some calls in this function rely on the old version of status
            self.lfstatus = False
            ctx1 = self[node1]
            ctx2 = self[node2]
            working = ctx2.rev() is None
            parentworking = working and ctx1 == self['.']

            if match is None:
                match = match_.always(self.root, self.getcwd())

            wlock = None
            try:
                try:
                    # updating the dirstate is optional
                    # so we don't wait on the lock
                    wlock = self.wlock(False)
                except error.LockError:
                    pass

                # First check if paths or patterns were specified on the
                # command line.  If there were, and they don't match any
                # largefiles, we should just bail here and let super
                # handle it -- thus gaining a big performance boost.
                lfdirstate = lfutil.openlfdirstate(ui, self)
                if not match.always():
                    for f in lfdirstate:
                        if match(f):
                            break
                    else:
                        return orig(node1, node2, match, listignored, listclean,
                                    listunknown, listsubrepos)

                # Create a copy of match that matches standins instead
                # of largefiles.
                def tostandins(files):
                    if not working:
                        return files
                    newfiles = []
                    dirstate = self.dirstate
                    for f in files:
                        sf = lfutil.standin(f)
                        if sf in dirstate:
                            newfiles.append(sf)
                        elif sf in dirstate.dirs():
                            # Directory entries could be regular or
                            # standin, check both
                            newfiles.extend((f, sf))
                        else:
                            newfiles.append(f)
                    return newfiles

                m = copy.copy(match)
                m._files = tostandins(m._files)

                result = orig(node1, node2, m, ignored, clean, unknown,
                              listsubrepos)
                if working:

                    def sfindirstate(f):
                        sf = lfutil.standin(f)
                        dirstate = self.dirstate
                        return sf in dirstate or sf in dirstate.dirs()

                    match._files = [f for f in match._files
                                    if sfindirstate(f)]
                    # Don't waste time getting the ignored and unknown
                    # files from lfdirstate
                    unsure, s = lfdirstate.status(match, [], False, listclean,
                                                  False)
                    (modified, added, removed, clean) = (s.modified, s.added,
                                                         s.removed, s.clean)
                    if parentworking:
                        for lfile in unsure:
                            standin = lfutil.standin(lfile)
                            if standin not in ctx1:
                                # from second parent
                                modified.append(lfile)
                            elif ctx1[standin].data().strip() \
                                    != lfutil.hashfile(self.wjoin(lfile)):
                                modified.append(lfile)
                            else:
                                if listclean:
                                    clean.append(lfile)
                                lfdirstate.normal(lfile)
                    else:
                        tocheck = unsure + modified + added + clean
                        modified, added, clean = [], [], []
                        checkexec = self.dirstate._checkexec

                        for lfile in tocheck:
                            standin = lfutil.standin(lfile)
                            if standin in ctx1:
                                abslfile = self.wjoin(lfile)
                                if ((ctx1[standin].data().strip() !=
                                     lfutil.hashfile(abslfile)) or
                                    (checkexec and
                                     ('x' in ctx1.flags(standin)) !=
                                     bool(lfutil.getexecutable(abslfile)))):
                                    modified.append(lfile)
                                elif listclean:
                                    clean.append(lfile)
                            else:
                                added.append(lfile)

                        # at this point, 'removed' contains largefiles
                        # marked as 'R' in the working context.
                        # then, largefiles not managed also in the target
                        # context should be excluded from 'removed'.
                        removed = [lfile for lfile in removed
                                   if lfutil.standin(lfile) in ctx1]

                    # Standins no longer found in lfdirstate has been
                    # removed
                    for standin in ctx1.walk(lfutil.getstandinmatcher(self)):
                        lfile = lfutil.splitstandin(standin)
                        if not match(lfile):
                            continue
                        if lfile not in lfdirstate:
                            removed.append(lfile)

                    # Filter result lists
                    result = list(result)

                    # Largefiles are not really removed when they're
                    # still in the normal dirstate. Likewise, normal
                    # files are not really removed if they are still in
                    # lfdirstate. This happens in merges where files
                    # change type.
                    removed = [f for f in removed
                               if f not in self.dirstate]
                    result[2] = [f for f in result[2]
                                 if f not in lfdirstate]

                    lfiles = set(lfdirstate._map)
                    # Unknown files
                    result[4] = set(result[4]).difference(lfiles)
                    # Ignored files
                    result[5] = set(result[5]).difference(lfiles)
                    # combine normal files and largefiles
                    normals = [[fn for fn in filelist
                                if not lfutil.isstandin(fn)]
                               for filelist in result]
                    lfstatus = (modified, added, removed, s.deleted, [], [],
                                clean)
                    result = [sorted(list1 + list2)
                              for (list1, list2) in zip(normals, lfstatus)]
                else: # not against working directory
                    result = [[lfutil.splitstandin(f) or f for f in items]
                              for items in result]

                if wlock:
                    lfdirstate.write()

            finally:
                if wlock:
                    wlock.release()

            self.lfstatus = True
            return scmutil.status(*result)

        def commitctx(self, ctx, *args, **kwargs):
            node = super(lfilesrepo, self).commitctx(ctx, *args, **kwargs)
            class lfilesctx(ctx.__class__):
                def markcommitted(self, node):
                    orig = super(lfilesctx, self).markcommitted
                    return lfutil.markcommitted(orig, self, node)
            ctx.__class__ = lfilesctx
            return node

        # Before commit, largefile standins have not had their
        # contents updated to reflect the hash of their largefile.
        # Do that here.
        def commit(self, text="", user=None, date=None, match=None,
                force=False, editor=False, extra={}):
            orig = super(lfilesrepo, self).commit

            wlock = self.wlock()
            try:
                lfcommithook = self._lfcommithooks[-1]
                match = lfcommithook(self, match)
                result = orig(text=text, user=user, date=date, match=match,
                                force=force, editor=editor, extra=extra)
                return result
            finally:
                wlock.release()

        def push(self, remote, force=False, revs=None, newbranch=False):
            if remote.local():
                missing = set(self.requirements) - remote.local().supported
                if missing:
                    msg = _("required features are not"
                            " supported in the destination:"
                            " %s") % (', '.join(sorted(missing)))
                    raise util.Abort(msg)
            return super(lfilesrepo, self).push(remote, force=force, revs=revs,
                newbranch=newbranch)

        # TODO: _subdirlfs should be moved into "lfutil.py", because
        # it is referred only from "lfutil.updatestandinsbymatch"
        def _subdirlfs(self, files, lfiles):
            '''
            Adjust matched file list
            If we pass a directory to commit whose only committable files
            are largefiles, the core commit code aborts before finding
            the largefiles.
            So we do the following:
            For directories that only have largefiles as matches,
            we explicitly add the largefiles to the match list and remove
            the directory.
            In other cases, we leave the match list unmodified.
            '''
            actualfiles = []
            dirs = []
            regulars = []

            for f in files:
                if lfutil.isstandin(f + '/'):
                    raise util.Abort(
                        _('file "%s" is a largefile standin') % f,
                        hint=('commit the largefile itself instead'))
                # Scan directories
                if os.path.isdir(self.wjoin(f)):
                    dirs.append(f)
                else:
                    regulars.append(f)

            for f in dirs:
                matcheddir = False
                d = self.dirstate.normalize(f) + '/'
                # Check for matched normal files
                for mf in regulars:
                    if self.dirstate.normalize(mf).startswith(d):
                        actualfiles.append(f)
                        matcheddir = True
                        break
                if not matcheddir:
                    # If no normal match, manually append
                    # any matching largefiles
                    for lf in lfiles:
                        if self.dirstate.normalize(lf).startswith(d):
                            actualfiles.append(lf)
                            if not matcheddir:
                                # There may still be normal files in the dir, so
                                # add a directory to the list, which
                                # forces status/dirstate to walk all files and
                                # call the match function on the matcher, even
                                # on case sensitive filesystems.
                                actualfiles.append('.')
                                matcheddir = True
                # Nothing in dir, so readd it
                # and let commit reject it
                if not matcheddir:
                    actualfiles.append(f)

            # Always add normal files
            actualfiles += regulars
            return actualfiles

    repo.__class__ = lfilesrepo

    # stack of hooks being executed before committing.
    # only last element ("_lfcommithooks[-1]") is used for each committing.
    repo._lfcommithooks = [lfutil.updatestandinsbymatch]

    # Stack of status writer functions taking "*msg, **opts" arguments
    # like "ui.status()". Only last element ("_lfstatuswriters[-1]")
    # is used to write status out.
    repo._lfstatuswriters = [ui.status]

    def prepushoutgoinghook(local, remote, outgoing):
        if outgoing.missing:
            toupload = set()
            addfunc = lambda fn, lfhash: toupload.add(lfhash)
            lfutil.getlfilestoupload(local, outgoing.missing, addfunc)
            lfcommands.uploadlfiles(ui, local, remote, toupload)
    repo.prepushoutgoinghooks.add("largefiles", prepushoutgoinghook)

    def checkrequireslfiles(ui, repo, **kwargs):
        if 'largefiles' not in repo.requirements and any(
                lfutil.shortname+'/' in f[0] for f in repo.store.datafiles()):
            repo.requirements.add('largefiles')
            repo._writerequirements()

    ui.setconfig('hooks', 'changegroup.lfiles', checkrequireslfiles,
                 'largefiles')
    ui.setconfig('hooks', 'commit.lfiles', checkrequireslfiles, 'largefiles')
