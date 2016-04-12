# hg.py - hg backend for convert extension
#
#  Copyright 2005-2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# Notes for hg->hg conversion:
#
# * Old versions of Mercurial didn't trim the whitespace from the ends
#   of commit messages, but new versions do.  Changesets created by
#   those older versions, then converted, may thus have different
#   hashes for changesets that are otherwise identical.
#
# * Using "--config convert.hg.saverev=true" will make the source
#   identifier to be stored in the converted revision. This will cause
#   the converted revision to have a different identity than the
#   source.
from __future__ import absolute_import

import os
import re
import time

from mercurial import (
    bookmarks,
    context,
    error,
    exchange,
    hg,
    lock as lockmod,
    merge as mergemod,
    node as nodemod,
    phases,
    scmutil,
    util,
)
stringio = util.stringio

from mercurial.i18n import _
from . import common
mapfile = common.mapfile
NoRepo = common.NoRepo

sha1re = re.compile(r'\b[0-9a-f]{12,40}\b')

class mercurial_sink(common.converter_sink):
    def __init__(self, ui, path):
        common.converter_sink.__init__(self, ui, path)
        self.branchnames = ui.configbool('convert', 'hg.usebranchnames', True)
        self.clonebranches = ui.configbool('convert', 'hg.clonebranches', False)
        self.tagsbranch = ui.config('convert', 'hg.tagsbranch', 'default')
        self.lastbranch = None
        if os.path.isdir(path) and len(os.listdir(path)) > 0:
            try:
                self.repo = hg.repository(self.ui, path)
                if not self.repo.local():
                    raise NoRepo(_('%s is not a local Mercurial repository')
                                 % path)
            except error.RepoError as err:
                ui.traceback()
                raise NoRepo(err.args[0])
        else:
            try:
                ui.status(_('initializing destination %s repository\n') % path)
                self.repo = hg.repository(self.ui, path, create=True)
                if not self.repo.local():
                    raise NoRepo(_('%s is not a local Mercurial repository')
                                 % path)
                self.created.append(path)
            except error.RepoError:
                ui.traceback()
                raise NoRepo(_("could not create hg repository %s as sink")
                             % path)
        self.lock = None
        self.wlock = None
        self.filemapmode = False
        self.subrevmaps = {}

    def before(self):
        self.ui.debug('run hg sink pre-conversion action\n')
        self.wlock = self.repo.wlock()
        self.lock = self.repo.lock()

    def after(self):
        self.ui.debug('run hg sink post-conversion action\n')
        if self.lock:
            self.lock.release()
        if self.wlock:
            self.wlock.release()

    def revmapfile(self):
        return self.repo.join("shamap")

    def authorfile(self):
        return self.repo.join("authormap")

    def setbranch(self, branch, pbranches):
        if not self.clonebranches:
            return

        setbranch = (branch != self.lastbranch)
        self.lastbranch = branch
        if not branch:
            branch = 'default'
        pbranches = [(b[0], b[1] and b[1] or 'default') for b in pbranches]
        if pbranches:
            pbranch = pbranches[0][1]
        else:
            pbranch = 'default'

        branchpath = os.path.join(self.path, branch)
        if setbranch:
            self.after()
            try:
                self.repo = hg.repository(self.ui, branchpath)
            except Exception:
                self.repo = hg.repository(self.ui, branchpath, create=True)
            self.before()

        # pbranches may bring revisions from other branches (merge parents)
        # Make sure we have them, or pull them.
        missings = {}
        for b in pbranches:
            try:
                self.repo.lookup(b[0])
            except Exception:
                missings.setdefault(b[1], []).append(b[0])

        if missings:
            self.after()
            for pbranch, heads in sorted(missings.iteritems()):
                pbranchpath = os.path.join(self.path, pbranch)
                prepo = hg.peer(self.ui, {}, pbranchpath)
                self.ui.note(_('pulling from %s into %s\n') % (pbranch, branch))
                exchange.pull(self.repo, prepo,
                              [prepo.lookup(h) for h in heads])
            self.before()

    def _rewritetags(self, source, revmap, data):
        fp = stringio()
        for line in data.splitlines():
            s = line.split(' ', 1)
            if len(s) != 2:
                continue
            revid = revmap.get(source.lookuprev(s[0]))
            if not revid:
                if s[0] == nodemod.nullhex:
                    revid = s[0]
                else:
                    continue
            fp.write('%s %s\n' % (revid, s[1]))
        return fp.getvalue()

    def _rewritesubstate(self, source, data):
        fp = stringio()
        for line in data.splitlines():
            s = line.split(' ', 1)
            if len(s) != 2:
                continue

            revid = s[0]
            subpath = s[1]
            if revid != nodemod.nullhex:
                revmap = self.subrevmaps.get(subpath)
                if revmap is None:
                    revmap = mapfile(self.ui,
                                     self.repo.wjoin(subpath, '.hg/shamap'))
                    self.subrevmaps[subpath] = revmap

                    # It is reasonable that one or more of the subrepos don't
                    # need to be converted, in which case they can be cloned
                    # into place instead of converted.  Therefore, only warn
                    # once.
                    msg = _('no ".hgsubstate" updates will be made for "%s"\n')
                    if len(revmap) == 0:
                        sub = self.repo.wvfs.reljoin(subpath, '.hg')

                        if self.repo.wvfs.exists(sub):
                            self.ui.warn(msg % subpath)

                newid = revmap.get(revid)
                if not newid:
                    if len(revmap) > 0:
                        self.ui.warn(_("%s is missing from %s/.hg/shamap\n") %
                                     (revid, subpath))
                else:
                    revid = newid

            fp.write('%s %s\n' % (revid, subpath))

        return fp.getvalue()

    def _calculatemergedfiles(self, source, p1ctx, p2ctx):
        """Calculates the files from p2 that we need to pull in when merging p1
        and p2, given that the merge is coming from the given source.

        This prevents us from losing files that only exist in the target p2 and
        that don't come from the source repo (like if you're merging multiple
        repositories together).
        """
        anc = [p1ctx.ancestor(p2ctx)]
        # Calculate what files are coming from p2
        actions, diverge, rename = mergemod.calculateupdates(
            self.repo, p1ctx, p2ctx, anc,
            True,  # branchmerge
            True,  # force
            False, # acceptremote
            False, # followcopies
        )

        for file, (action, info, msg) in actions.iteritems():
            if source.targetfilebelongstosource(file):
                # If the file belongs to the source repo, ignore the p2
                # since it will be covered by the existing fileset.
                continue

            # If the file requires actual merging, abort. We don't have enough
            # context to resolve merges correctly.
            if action in ['m', 'dm', 'cd', 'dc']:
                raise error.Abort(_("unable to convert merge commit "
                    "since target parents do not merge cleanly (file "
                    "%s, parents %s and %s)") % (file, p1ctx,
                                                 p2ctx))
            elif action == 'k':
                # 'keep' means nothing changed from p1
                continue
            else:
                # Any other change means we want to take the p2 version
                yield file

    def putcommit(self, files, copies, parents, commit, source, revmap, full,
                  cleanp2):
        files = dict(files)

        def getfilectx(repo, memctx, f):
            if p2ctx and f in p2files and f not in copies:
                self.ui.debug('reusing %s from p2\n' % f)
                try:
                    return p2ctx[f]
                except error.ManifestLookupError:
                    # If the file doesn't exist in p2, then we're syncing a
                    # delete, so just return None.
                    return None
            try:
                v = files[f]
            except KeyError:
                return None
            data, mode = source.getfile(f, v)
            if data is None:
                return None
            if f == '.hgtags':
                data = self._rewritetags(source, revmap, data)
            if f == '.hgsubstate':
                data = self._rewritesubstate(source, data)
            return context.memfilectx(self.repo, f, data, 'l' in mode,
                                      'x' in mode, copies.get(f))

        pl = []
        for p in parents:
            if p not in pl:
                pl.append(p)
        parents = pl
        nparents = len(parents)
        if self.filemapmode and nparents == 1:
            m1node = self.repo.changelog.read(nodemod.bin(parents[0]))[0]
            parent = parents[0]

        if len(parents) < 2:
            parents.append(nodemod.nullid)
        if len(parents) < 2:
            parents.append(nodemod.nullid)
        p2 = parents.pop(0)

        text = commit.desc

        sha1s = re.findall(sha1re, text)
        for sha1 in sha1s:
            oldrev = source.lookuprev(sha1)
            newrev = revmap.get(oldrev)
            if newrev is not None:
                text = text.replace(sha1, newrev[:len(sha1)])

        extra = commit.extra.copy()

        sourcename = self.repo.ui.config('convert', 'hg.sourcename')
        if sourcename:
            extra['convert_source'] = sourcename

        for label in ('source', 'transplant_source', 'rebase_source',
                      'intermediate-source'):
            node = extra.get(label)

            if node is None:
                continue

            # Only transplant stores its reference in binary
            if label == 'transplant_source':
                node = nodemod.hex(node)

            newrev = revmap.get(node)
            if newrev is not None:
                if label == 'transplant_source':
                    newrev = nodemod.bin(newrev)

                extra[label] = newrev

        if self.branchnames and commit.branch:
            extra['branch'] = commit.branch
        if commit.rev and commit.saverev:
            extra['convert_revision'] = commit.rev

        while parents:
            p1 = p2
            p2 = parents.pop(0)
            p1ctx = self.repo[p1]
            p2ctx = None
            if p2 != nodemod.nullid:
                p2ctx = self.repo[p2]
            fileset = set(files)
            if full:
                fileset.update(self.repo[p1])
                fileset.update(self.repo[p2])

            if p2ctx:
                p2files = set(cleanp2)
                for file in self._calculatemergedfiles(source, p1ctx, p2ctx):
                    p2files.add(file)
                    fileset.add(file)

            ctx = context.memctx(self.repo, (p1, p2), text, fileset,
                                 getfilectx, commit.author, commit.date, extra)

            # We won't know if the conversion changes the node until after the
            # commit, so copy the source's phase for now.
            self.repo.ui.setconfig('phases', 'new-commit',
                                   phases.phasenames[commit.phase], 'convert')

            with self.repo.transaction("convert") as tr:
                node = nodemod.hex(self.repo.commitctx(ctx))

                # If the node value has changed, but the phase is lower than
                # draft, set it back to draft since it hasn't been exposed
                # anywhere.
                if commit.rev != node:
                    ctx = self.repo[node]
                    if ctx.phase() < phases.draft:
                        phases.retractboundary(self.repo, tr, phases.draft,
                                               [ctx.node()])

            text = "(octopus merge fixup)\n"
            p2 = node

        if self.filemapmode and nparents == 1:
            man = self.repo.manifest
            mnode = self.repo.changelog.read(nodemod.bin(p2))[0]
            closed = 'close' in commit.extra
            if not closed and not man.cmp(m1node, man.revision(mnode)):
                self.ui.status(_("filtering out empty revision\n"))
                self.repo.rollback(force=True)
                return parent
        return p2

    def puttags(self, tags):
        try:
            parentctx = self.repo[self.tagsbranch]
            tagparent = parentctx.node()
        except error.RepoError:
            parentctx = None
            tagparent = nodemod.nullid

        oldlines = set()
        for branch, heads in self.repo.branchmap().iteritems():
            for h in heads:
                if '.hgtags' in self.repo[h]:
                    oldlines.update(
                        set(self.repo[h]['.hgtags'].data().splitlines(True)))
        oldlines = sorted(list(oldlines))

        newlines = sorted([("%s %s\n" % (tags[tag], tag)) for tag in tags])
        if newlines == oldlines:
            return None, None

        # if the old and new tags match, then there is nothing to update
        oldtags = set()
        newtags = set()
        for line in oldlines:
            s = line.strip().split(' ', 1)
            if len(s) != 2:
                continue
            oldtags.add(s[1])
        for line in newlines:
            s = line.strip().split(' ', 1)
            if len(s) != 2:
                continue
            if s[1] not in oldtags:
                newtags.add(s[1].strip())

        if not newtags:
            return None, None

        data = "".join(newlines)
        def getfilectx(repo, memctx, f):
            return context.memfilectx(repo, f, data, False, False, None)

        self.ui.status(_("updating tags\n"))
        date = "%s 0" % int(time.mktime(time.gmtime()))
        extra = {'branch': self.tagsbranch}
        ctx = context.memctx(self.repo, (tagparent, None), "update tags",
                             [".hgtags"], getfilectx, "convert-repo", date,
                             extra)
        node = self.repo.commitctx(ctx)
        return nodemod.hex(node), nodemod.hex(tagparent)

    def setfilemapmode(self, active):
        self.filemapmode = active

    def putbookmarks(self, updatedbookmark):
        if not len(updatedbookmark):
            return
        wlock = lock = tr = None
        try:
            wlock = self.repo.wlock()
            lock = self.repo.lock()
            tr = self.repo.transaction('bookmark')
            self.ui.status(_("updating bookmarks\n"))
            destmarks = self.repo._bookmarks
            for bookmark in updatedbookmark:
                destmarks[bookmark] = nodemod.bin(updatedbookmark[bookmark])
            destmarks.recordchange(tr)
            tr.close()
        finally:
            lockmod.release(lock, wlock, tr)

    def hascommitfrommap(self, rev):
        # the exact semantics of clonebranches is unclear so we can't say no
        return rev in self.repo or self.clonebranches

    def hascommitforsplicemap(self, rev):
        if rev not in self.repo and self.clonebranches:
            raise error.Abort(_('revision %s not found in destination '
                               'repository (lookups with clonebranches=true '
                               'are not implemented)') % rev)
        return rev in self.repo

class mercurial_source(common.converter_source):
    def __init__(self, ui, path, revs=None):
        common.converter_source.__init__(self, ui, path, revs)
        self.ignoreerrors = ui.configbool('convert', 'hg.ignoreerrors', False)
        self.ignored = set()
        self.saverev = ui.configbool('convert', 'hg.saverev', False)
        try:
            self.repo = hg.repository(self.ui, path)
            # try to provoke an exception if this isn't really a hg
            # repo, but some other bogus compatible-looking url
            if not self.repo.local():
                raise error.RepoError
        except error.RepoError:
            ui.traceback()
            raise NoRepo(_("%s is not a local Mercurial repository") % path)
        self.lastrev = None
        self.lastctx = None
        self._changescache = None, None
        self.convertfp = None
        # Restrict converted revisions to startrev descendants
        startnode = ui.config('convert', 'hg.startrev')
        hgrevs = ui.config('convert', 'hg.revs')
        if hgrevs is None:
            if startnode is not None:
                try:
                    startnode = self.repo.lookup(startnode)
                except error.RepoError:
                    raise error.Abort(_('%s is not a valid start revision')
                                     % startnode)
                startrev = self.repo.changelog.rev(startnode)
                children = {startnode: 1}
                for r in self.repo.changelog.descendants([startrev]):
                    children[self.repo.changelog.node(r)] = 1
                self.keep = children.__contains__
            else:
                self.keep = util.always
            if revs:
                self._heads = [self.repo[r].node() for r in revs]
            else:
                self._heads = self.repo.heads()
        else:
            if revs or startnode is not None:
                raise error.Abort(_('hg.revs cannot be combined with '
                                   'hg.startrev or --rev'))
            nodes = set()
            parents = set()
            for r in scmutil.revrange(self.repo, [hgrevs]):
                ctx = self.repo[r]
                nodes.add(ctx.node())
                parents.update(p.node() for p in ctx.parents())
            self.keep = nodes.__contains__
            self._heads = nodes - parents

    def _changectx(self, rev):
        if self.lastrev != rev:
            self.lastctx = self.repo[rev]
            self.lastrev = rev
        return self.lastctx

    def _parents(self, ctx):
        return [p for p in ctx.parents() if p and self.keep(p.node())]

    def getheads(self):
        return [nodemod.hex(h) for h in self._heads if self.keep(h)]

    def getfile(self, name, rev):
        try:
            fctx = self._changectx(rev)[name]
            return fctx.data(), fctx.flags()
        except error.LookupError:
            return None, None

    def _changedfiles(self, ctx1, ctx2):
        ma, r = [], []
        maappend = ma.append
        rappend = r.append
        d = ctx1.manifest().diff(ctx2.manifest())
        for f, ((node1, flag1), (node2, flag2)) in d.iteritems():
            if node2 is None:
                rappend(f)
            else:
                maappend(f)
        return ma, r

    def getchanges(self, rev, full):
        ctx = self._changectx(rev)
        parents = self._parents(ctx)
        if full or not parents:
            files = copyfiles = ctx.manifest()
        if parents:
            if self._changescache[0] == rev:
                ma, r = self._changescache[1]
            else:
                ma, r = self._changedfiles(parents[0], ctx)
            if not full:
                files = ma + r
            copyfiles = ma
        # _getcopies() is also run for roots and before filtering so missing
        # revlogs are detected early
        copies = self._getcopies(ctx, parents, copyfiles)
        cleanp2 = set()
        if len(parents) == 2:
            d = parents[1].manifest().diff(ctx.manifest(), clean=True)
            for f, value in d.iteritems():
                if value is None:
                    cleanp2.add(f)
        changes = [(f, rev) for f in files if f not in self.ignored]
        changes.sort()
        return changes, copies, cleanp2

    def _getcopies(self, ctx, parents, files):
        copies = {}
        for name in files:
            if name in self.ignored:
                continue
            try:
                copysource, _copynode = ctx.filectx(name).renamed()
                if copysource in self.ignored:
                    continue
                # Ignore copy sources not in parent revisions
                found = False
                for p in parents:
                    if copysource in p:
                        found = True
                        break
                if not found:
                    continue
                copies[name] = copysource
            except TypeError:
                pass
            except error.LookupError as e:
                if not self.ignoreerrors:
                    raise
                self.ignored.add(name)
                self.ui.warn(_('ignoring: %s\n') % e)
        return copies

    def getcommit(self, rev):
        ctx = self._changectx(rev)
        _parents = self._parents(ctx)
        parents = [p.hex() for p in _parents]
        optparents = [p.hex() for p in ctx.parents() if p and p not in _parents]
        crev = rev

        return common.commit(author=ctx.user(),
                             date=util.datestr(ctx.date(),
                                               '%Y-%m-%d %H:%M:%S %1%2'),
                             desc=ctx.description(),
                             rev=crev,
                             parents=parents,
                             optparents=optparents,
                             branch=ctx.branch(),
                             extra=ctx.extra(),
                             sortkey=ctx.rev(),
                             saverev=self.saverev,
                             phase=ctx.phase())

    def gettags(self):
        # This will get written to .hgtags, filter non global tags out.
        tags = [t for t in self.repo.tagslist()
                if self.repo.tagtype(t[0]) == 'global']
        return dict([(name, nodemod.hex(node)) for name, node in tags
                     if self.keep(node)])

    def getchangedfiles(self, rev, i):
        ctx = self._changectx(rev)
        parents = self._parents(ctx)
        if not parents and i is None:
            i = 0
            ma, r = ctx.manifest().keys(), []
        else:
            i = i or 0
            ma, r = self._changedfiles(parents[i], ctx)
        ma, r = [[f for f in l if f not in self.ignored] for l in (ma, r)]

        if i == 0:
            self._changescache = (rev, (ma, r))

        return ma + r

    def converted(self, rev, destrev):
        if self.convertfp is None:
            self.convertfp = open(self.repo.join('shamap'), 'a')
        self.convertfp.write('%s %s\n' % (destrev, rev))
        self.convertfp.flush()

    def before(self):
        self.ui.debug('run hg source pre-conversion action\n')

    def after(self):
        self.ui.debug('run hg source post-conversion action\n')

    def hasnativeorder(self):
        return True

    def hasnativeclose(self):
        return True

    def lookuprev(self, rev):
        try:
            return nodemod.hex(self.repo.lookup(rev))
        except (error.RepoError, error.LookupError):
            return None

    def getbookmarks(self):
        return bookmarks.listbookmarks(self.repo)

    def checkrevformat(self, revstr, mapname='splicemap'):
        """ Mercurial, revision string is a 40 byte hex """
        self.checkhexformat(revstr, mapname)
