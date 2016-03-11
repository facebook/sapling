# verify.py - repository integrity checking for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

from .i18n import _
from .node import (
    nullid,
    short,
)

from . import (
    error,
    revlog,
    util,
)

def verify(repo):
    with repo.lock():
        return verifier(repo).verify()

def _normpath(f):
    # under hg < 2.4, convert didn't sanitize paths properly, so a
    # converted repo may contain repeated slashes
    while '//' in f:
        f = f.replace('//', '/')
    return f

def _validpath(repo, path):
    """Returns False if a path should NOT be treated as part of a repo.

    For all in-core cases, this returns True, as we have no way for a
    path to be mentioned in the history but not actually be
    relevant. For narrow clones, this is important because many
    filelogs will be missing, and changelog entries may mention
    modified files that are outside the narrow scope.
    """
    return True

class verifier(object):
    def __init__(self, repo):
        self.repo = repo.unfiltered()
        self.ui = repo.ui
        self.badrevs = set()
        self.errors = 0
        self.warnings = 0
        self.havecl = len(repo.changelog) > 0
        self.havemf = len(repo.manifest) > 0
        self.revlogv1 = repo.changelog.version != revlog.REVLOGV0
        self.lrugetctx = util.lrucachefunc(repo.changectx)
        self.refersmf = False
        self.fncachewarned = False

    def warn(self, msg):
        self.ui.warn(msg + "\n")
        self.warnings += 1

    def err(self, linkrev, msg, filename=None):
        if linkrev is not None:
            self.badrevs.add(linkrev)
        else:
            linkrev = '?'
        msg = "%s: %s" % (linkrev, msg)
        if filename:
            msg = "%s@%s" % (filename, msg)
        self.ui.warn(" " + msg + "\n")
        self.errors += 1

    def exc(self, linkrev, msg, inst, filename=None):
        if not str(inst):
            inst = repr(inst)
        self.err(linkrev, "%s: %s" % (msg, inst), filename)

    def checklog(self, obj, name, linkrev):
        if not len(obj) and (self.havecl or self.havemf):
            self.err(linkrev, _("empty or missing %s") % name)
            return

        d = obj.checksize()
        if d[0]:
            self.err(None, _("data length off by %d bytes") % d[0], name)
        if d[1]:
            self.err(None, _("index contains %d extra bytes") % d[1], name)

        if obj.version != revlog.REVLOGV0:
            if not self.revlogv1:
                self.warn(_("warning: `%s' uses revlog format 1") % name)
        elif self.revlogv1:
            self.warn(_("warning: `%s' uses revlog format 0") % name)

    def checkentry(self, obj, i, node, seen, linkrevs, f):
        lr = obj.linkrev(obj.rev(node))
        if lr < 0 or (self.havecl and lr not in linkrevs):
            if lr < 0 or lr >= len(self.repo.changelog):
                msg = _("rev %d points to nonexistent changeset %d")
            else:
                msg = _("rev %d points to unexpected changeset %d")
            self.err(None, msg % (i, lr), f)
            if linkrevs:
                if f and len(linkrevs) > 1:
                    try:
                        # attempt to filter down to real linkrevs
                        linkrevs = [l for l in linkrevs
                                    if self.lrugetctx(l)[f].filenode() == node]
                    except Exception:
                        pass
                self.warn(_(" (expected %s)") % " ".join(map(str, linkrevs)))
            lr = None # can't be trusted

        try:
            p1, p2 = obj.parents(node)
            if p1 not in seen and p1 != nullid:
                self.err(lr, _("unknown parent 1 %s of %s") %
                    (short(p1), short(node)), f)
            if p2 not in seen and p2 != nullid:
                self.err(lr, _("unknown parent 2 %s of %s") %
                    (short(p2), short(node)), f)
        except Exception as inst:
            self.exc(lr, _("checking parents of %s") % short(node), inst, f)

        if node in seen:
            self.err(lr, _("duplicate revision %d (%d)") % (i, seen[node]), f)
        seen[node] = i
        return lr

    def verify(self):
        repo = self.repo

        ui = repo.ui

        if not repo.url().startswith('file:'):
            raise error.Abort(_("cannot verify bundle or remote repos"))

        if os.path.exists(repo.sjoin("journal")):
            ui.warn(_("abandoned transaction found - run hg recover\n"))

        if ui.verbose or not self.revlogv1:
            ui.status(_("repository uses revlog format %d\n") %
                           (self.revlogv1 and 1 or 0))

        mflinkrevs, filelinkrevs = self._verifychangelog()

        filenodes = self._verifymanifest(mflinkrevs)
        del mflinkrevs

        self._crosscheckfiles(filelinkrevs, filenodes)

        totalfiles, filerevisions = self._verifyfiles(filenodes, filelinkrevs)

        ui.status(_("%d files, %d changesets, %d total revisions\n") %
                       (totalfiles, len(repo.changelog), filerevisions))
        if self.warnings:
            ui.warn(_("%d warnings encountered!\n") % self.warnings)
        if self.fncachewarned:
            ui.warn(_('hint: run "hg debugrebuildfncache" to recover from '
                      'corrupt fncache\n'))
        if self.errors:
            ui.warn(_("%d integrity errors encountered!\n") % self.errors)
            if self.badrevs:
                ui.warn(_("(first damaged changeset appears to be %d)\n")
                        % min(self.badrevs))
            return 1

    def _verifychangelog(self):
        ui = self.ui
        repo = self.repo
        cl = repo.changelog

        ui.status(_("checking changesets\n"))
        mflinkrevs = {}
        filelinkrevs = {}
        seen = {}
        self.checklog(cl, "changelog", 0)
        total = len(repo)
        for i in repo:
            ui.progress(_('checking'), i, total=total, unit=_('changesets'))
            n = cl.node(i)
            self.checkentry(cl, i, n, seen, [i], "changelog")

            try:
                changes = cl.read(n)
                if changes[0] != nullid:
                    mflinkrevs.setdefault(changes[0], []).append(i)
                    self.refersmf = True
                for f in changes[3]:
                    if _validpath(repo, f):
                        filelinkrevs.setdefault(_normpath(f), []).append(i)
            except Exception as inst:
                self.refersmf = True
                self.exc(i, _("unpacking changeset %s") % short(n), inst)
        ui.progress(_('checking'), None)
        return mflinkrevs, filelinkrevs

    def _verifymanifest(self, mflinkrevs, dir="", storefiles=None,
                        progress=None):
        repo = self.repo
        ui = self.ui
        mf = self.repo.manifest.dirlog(dir)

        if not dir:
            self.ui.status(_("checking manifests\n"))

        filenodes = {}
        subdirnodes = {}
        seen = {}
        label = "manifest"
        if dir:
            label = dir
            revlogfiles = mf.files()
            storefiles.difference_update(revlogfiles)
            if progress: # should be true since we're in a subdirectory
                progress()
        if self.refersmf:
            # Do not check manifest if there are only changelog entries with
            # null manifests.
            self.checklog(mf, label, 0)
        total = len(mf)
        for i in mf:
            if not dir:
                ui.progress(_('checking'), i, total=total, unit=_('manifests'))
            n = mf.node(i)
            lr = self.checkentry(mf, i, n, seen, mflinkrevs.get(n, []), label)
            if n in mflinkrevs:
                del mflinkrevs[n]
            elif dir:
                self.err(lr, _("%s not in parent-directory manifest") %
                         short(n), label)
            else:
                self.err(lr, _("%s not in changesets") % short(n), label)

            try:
                for f, fn, fl in mf.readshallowdelta(n).iterentries():
                    if not f:
                        self.err(lr, _("entry without name in manifest"))
                    elif f == "/dev/null":  # ignore this in very old repos
                        continue
                    fullpath = dir + _normpath(f)
                    if not _validpath(repo, fullpath):
                        continue
                    if fl == 't':
                        subdirnodes.setdefault(fullpath + '/', {}).setdefault(
                            fn, []).append(lr)
                    else:
                        filenodes.setdefault(fullpath, {}).setdefault(fn, lr)
            except Exception as inst:
                self.exc(lr, _("reading delta %s") % short(n), inst, label)
        if not dir:
            ui.progress(_('checking'), None)

        if self.havemf:
            for c, m in sorted([(c, m) for m in mflinkrevs
                        for c in mflinkrevs[m]]):
                if dir:
                    self.err(c, _("parent-directory manifest refers to unknown "
                                  "revision %s") % short(m), label)
                else:
                    self.err(c, _("changeset refers to unknown revision %s") %
                             short(m), label)

        if not dir and subdirnodes:
            self.ui.status(_("checking directory manifests\n"))
            storefiles = set()
            subdirs = set()
            revlogv1 = self.revlogv1
            for f, f2, size in repo.store.datafiles():
                if not f:
                    self.err(None, _("cannot decode filename '%s'") % f2)
                elif (size > 0 or not revlogv1) and f.startswith('meta/'):
                    storefiles.add(_normpath(f))
                    subdirs.add(os.path.dirname(f))
            subdircount = len(subdirs)
            currentsubdir = [0]
            def progress():
                currentsubdir[0] += 1
                ui.progress(_('checking'), currentsubdir[0], total=subdircount,
                            unit=_('manifests'))

        for subdir, linkrevs in subdirnodes.iteritems():
            subdirfilenodes = self._verifymanifest(linkrevs, subdir, storefiles,
                                                   progress)
            for f, onefilenodes in subdirfilenodes.iteritems():
                filenodes.setdefault(f, {}).update(onefilenodes)

        if not dir and subdirnodes:
            ui.progress(_('checking'), None)
            for f in sorted(storefiles):
                self.warn(_("warning: orphan revlog '%s'") % f)

        return filenodes

    def _crosscheckfiles(self, filelinkrevs, filenodes):
        repo = self.repo
        ui = self.ui
        ui.status(_("crosschecking files in changesets and manifests\n"))

        total = len(filelinkrevs) + len(filenodes)
        count = 0
        if self.havemf:
            for f in sorted(filelinkrevs):
                count += 1
                ui.progress(_('crosschecking'), count, total=total)
                if f not in filenodes:
                    lr = filelinkrevs[f][0]
                    self.err(lr, _("in changeset but not in manifest"), f)

        if self.havecl:
            for f in sorted(filenodes):
                count += 1
                ui.progress(_('crosschecking'), count, total=total)
                if f not in filelinkrevs:
                    try:
                        fl = repo.file(f)
                        lr = min([fl.linkrev(fl.rev(n)) for n in filenodes[f]])
                    except Exception:
                        lr = None
                    self.err(lr, _("in manifest but not in changeset"), f)

        ui.progress(_('crosschecking'), None)

    def _verifyfiles(self, filenodes, filelinkrevs):
        repo = self.repo
        ui = self.ui
        lrugetctx = self.lrugetctx
        revlogv1 = self.revlogv1
        havemf = self.havemf
        ui.status(_("checking files\n"))

        storefiles = set()
        for f, f2, size in repo.store.datafiles():
            if not f:
                self.err(None, _("cannot decode filename '%s'") % f2)
            elif (size > 0 or not revlogv1) and f.startswith('data/'):
                storefiles.add(_normpath(f))

        files = sorted(set(filenodes) | set(filelinkrevs))
        total = len(files)
        revisions = 0
        for i, f in enumerate(files):
            ui.progress(_('checking'), i, item=f, total=total, unit=_('files'))
            try:
                linkrevs = filelinkrevs[f]
            except KeyError:
                # in manifest but not in changelog
                linkrevs = []

            if linkrevs:
                lr = linkrevs[0]
            else:
                lr = None

            try:
                fl = repo.file(f)
            except error.RevlogError as e:
                self.err(lr, _("broken revlog! (%s)") % e, f)
                continue

            for ff in fl.files():
                try:
                    storefiles.remove(ff)
                except KeyError:
                    self.warn(_(" warning: revlog '%s' not in fncache!") % ff)
                    self.fncachewarned = True

            self.checklog(fl, f, lr)
            seen = {}
            rp = None
            for i in fl:
                revisions += 1
                n = fl.node(i)
                lr = self.checkentry(fl, i, n, seen, linkrevs, f)
                if f in filenodes:
                    if havemf and n not in filenodes[f]:
                        self.err(lr, _("%s not in manifests") % (short(n)), f)
                    else:
                        del filenodes[f][n]

                # verify contents
                try:
                    l = len(fl.read(n))
                    rp = fl.renamed(n)
                    if l != fl.size(i):
                        if len(fl.revision(n)) != fl.size(i):
                            self.err(lr, _("unpacked size is %s, %s expected") %
                                     (l, fl.size(i)), f)
                except error.CensoredNodeError:
                    # experimental config: censor.policy
                    if ui.config("censor", "policy", "abort") == "abort":
                        self.err(lr, _("censored file data"), f)
                except Exception as inst:
                    self.exc(lr, _("unpacking %s") % short(n), inst, f)

                # check renames
                try:
                    if rp:
                        if lr is not None and ui.verbose:
                            ctx = lrugetctx(lr)
                            found = False
                            for pctx in ctx.parents():
                                if rp[0] in pctx:
                                    found = True
                                    break
                            if not found:
                                self.warn(_("warning: copy source of '%s' not"
                                            " in parents of %s") % (f, ctx))
                        fl2 = repo.file(rp[0])
                        if not len(fl2):
                            self.err(lr, _("empty or missing copy source "
                                     "revlog %s:%s") % (rp[0], short(rp[1])), f)
                        elif rp[1] == nullid:
                            ui.note(_("warning: %s@%s: copy source"
                                      " revision is nullid %s:%s\n")
                                % (f, lr, rp[0], short(rp[1])))
                        else:
                            fl2.rev(rp[1])
                except Exception as inst:
                    self.exc(lr, _("checking rename of %s") % short(n), inst, f)

            # cross-check
            if f in filenodes:
                fns = [(lr, n) for n, lr in filenodes[f].iteritems()]
                for lr, node in sorted(fns):
                    self.err(lr, _("manifest refers to unknown revision %s") %
                             short(node), f)
        ui.progress(_('checking'), None)

        for f in sorted(storefiles):
            self.warn(_("warning: orphan revlog '%s'") % f)

        return len(files), revisions
