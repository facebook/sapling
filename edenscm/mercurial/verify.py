# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# verify.py - repository integrity checking for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

from . import error, progress, revlog, scmutil, util
from .i18n import _
from .node import nullid, short


def verify(repo, revs=None):
    with repo.lock():
        return verifier(repo, revs=revs).verify()


def _normpath(f):
    # under hg < 2.4, convert didn't sanitize paths properly, so a
    # converted repo may contain repeated slashes
    while "//" in f:
        f = f.replace("//", "/")
    return f


class verifier(object):
    # The match argument is always None in hg core, but e.g. the narrowhg
    # extension will pass in a matcher here.
    def __init__(self, repo, match=None, revs=None):
        self.repo = repo.unfiltered()
        self.ui = repo.ui
        self.match = match or scmutil.matchall(repo)
        self.badrevs = set()
        self.errors = 0
        self.warnings = 0
        self.havecl = bool(repo.changelog)
        self.havemf = bool(repo.manifestlog)
        self.revlogv1 = repo.changelog.version != revlog.REVLOGV0
        self.lrugetctx = util.lrucachefunc(repo.changectx)
        self.refersmf = False
        self.fncachewarned = False
        # developer config: verify.skipflags
        self.skipflags = repo.ui.configint("verify", "skipflags")
        self.revs = revs

    def warn(self, msg):
        self.ui.warn(msg + "\n")
        self.warnings += 1

    def err(self, linkrev, msg, filename=None):
        if linkrev is not None:
            self.badrevs.add(linkrev)
        else:
            linkrev = "?"
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
                        linkrevs = [
                            l
                            for l in linkrevs
                            if self.lrugetctx(l)[f].filenode() == node
                        ]
                    except Exception:
                        pass
                self.warn(_(" (expected %s)") % " ".join(map(str, linkrevs)))
            lr = None  # can't be trusted

        if self.revs is None:
            try:
                p1, p2 = obj.parents(node)
                if p1 not in seen and p1 != nullid:
                    self.err(
                        lr, _("unknown parent 1 %s of %s") % (short(p1), short(node)), f
                    )
                if p2 not in seen and p2 != nullid:
                    self.err(
                        lr, _("unknown parent 2 %s of %s") % (short(p2), short(node)), f
                    )
            except Exception as inst:
                self.exc(lr, _("checking parents of %s") % short(node), inst, f)

        if node in seen:
            self.err(lr, _("duplicate revision %d (%d)") % (i, seen[node]), f)
        seen[node] = i
        return lr

    def verify(self):
        repo = self.repo

        ui = repo.ui

        if not repo.url().startswith("file:"):
            raise error.Abort(_("cannot verify bundle or remote repos"))

        if os.path.exists(repo.sjoin("journal")):
            ui.warn(_("abandoned transaction found - run hg recover\n"))

        if ui.verbose or not self.revlogv1:
            ui.status(
                _("repository uses revlog format %d\n") % (self.revlogv1 and 1 or 0)
            )

        mflinkrevs, filelinkrevs = self._verifychangelog()

        filenodes = self._verifymanifest(mflinkrevs)
        del mflinkrevs

        self._crosscheckfiles(filelinkrevs, filenodes)

        totalfiles, filerevisions = self._verifyfiles(filenodes, filelinkrevs)

        if self.revs is not None:
            totalchangesets = len(self.revs)
        else:
            totalchangesets = len(repo.changelog)
        ui.status(
            _("%d files, %d changesets, %d total revisions\n")
            % (totalfiles, totalchangesets, filerevisions)
        )
        if self.warnings:
            ui.warn(_("%d warnings encountered!\n") % self.warnings)
        if self.fncachewarned:
            ui.warn(
                _(
                    'hint: run "hg debugrebuildfncache" to recover from '
                    "corrupt fncache\n"
                )
            )
        if self.errors:
            ui.warn(_("%d integrity errors encountered!\n") % self.errors)
            if self.badrevs:
                ui.warn(
                    _("(first damaged changeset appears to be %d)\n")
                    % min(self.badrevs)
                )
            return 1

    def _verifychangelog(self):
        ui = self.ui
        repo = self.repo
        match = self.match
        cl = repo.changelog

        ui.status(_("checking changesets\n"))
        mflinkrevs = {}
        filelinkrevs = {}
        seen = {}

        if self.revs is not None:
            revs = self.revs
        else:
            revs = repo

        self.checklog(cl, "changelog", 0)
        total = len(revs)
        with progress.bar(ui, _("checking"), _("changesets"), total) as prog:
            for i in revs:
                prog.value = i
                n = cl.node(i)
                self.checkentry(cl, i, n, seen, [i], "changelog")

                try:
                    changes = cl.read(n)
                    if changes[0] != nullid:
                        mflinkrevs.setdefault(changes[0], []).append(i)
                        self.refersmf = True
                    for f in changes[3]:
                        if match(f):
                            filelinkrevs.setdefault(_normpath(f), []).append(i)
                except Exception as inst:
                    self.refersmf = True
                    self.exc(i, _("unpacking changeset %s") % short(n), inst)
        return mflinkrevs, filelinkrevs

    def _verifymanifest(self, mflinkrevs):
        if self.ui.configbool("verify", "skipmanifests", False):
            self.ui.warn(
                _(
                    "verify.skipmanifests is enabled; skipping "
                    "verification of manifests\n"
                )
            )
            return []

        self.ui.status(_("checking manifests\n"))

        with progress.bar(self.ui, _("checking"), _("manifests")) as prog:
            filenodes, subdirnodes = self._verifymanifestpart(mflinkrevs, progress=prog)

        if subdirnodes:
            self.ui.status(_("checking directory manifests\n"))
            storefiles = set()
            subdirs = set()
            revlogv1 = self.revlogv1
            for f, f2, size in self.repo.store.datafiles():
                if not f:
                    self.err(None, _("cannot decode filename '%s'") % f2)
                elif (size > 0 or not revlogv1) and f.startswith("meta/"):
                    storefiles.add(_normpath(f))
                    subdirs.add(os.path.dirname(f))
            subdircount = len(subdirs)
            with progress.bar(
                self.ui, _("checking"), _("manifests"), subdircount
            ) as prog:
                self._verifymanifesttree(filenodes, subdirnodes, storefiles, prog)

            for f in sorted(storefiles):
                self.warn(_("warning: orphan revlog '%s'") % f)

        return filenodes

    def _verifymanifestpart(self, mflinkrevs, dir="", storefiles=None, progress=None):

        match = self.match
        mfl = self.repo.manifestlog
        mf = mfl._revlog.dirlog(dir)

        filenodes = {}
        subdirnodes = {}
        seen = {}
        label = "manifest"
        if dir:
            label = dir
            revlogfiles = mf.files()
            storefiles.difference_update(revlogfiles)
        if self.refersmf:
            # Do not check manifest if there are only changelog entries with
            # null manifests.
            self.checklog(mf, label, 0)
        if progress:
            progress._total = len(mf)
        for i in mf:
            if self.revs is not None and mf.linkrev(i) not in self.revs:
                continue
            if progress:
                progress.value = i
            n = mf.node(i)
            lr = self.checkentry(mf, i, n, seen, mflinkrevs.get(n, []), label)
            if n in mflinkrevs:
                del mflinkrevs[n]
            elif dir:
                self.err(lr, _("%s not in parent-directory manifest") % short(n), label)
            else:
                self.err(lr, _("%s not in changesets") % short(n), label)

            try:
                mfdelta = mfl.get(dir, n).readnew(shallow=True)
                for f, fn, fl in mfdelta.iterentries():
                    if not f:
                        self.err(lr, _("entry without name in manifest"))
                    elif f == "/dev/null":  # ignore this in very old repos
                        continue
                    fullpath = dir + _normpath(f)
                    if fl == "t":
                        if not match.visitdir(fullpath):
                            continue
                        subdirnodes.setdefault(fullpath + "/", {}).setdefault(
                            fn, []
                        ).append(lr)
                    else:
                        if not match(fullpath):
                            continue
                        filenodes.setdefault(fullpath, {}).setdefault(fn, lr)
            except Exception as inst:
                self.exc(lr, _("reading delta %s") % short(n), inst, label)

        if self.havemf:
            for c, m in sorted([(c, m) for m in mflinkrevs for c in mflinkrevs[m]]):
                if dir:
                    self.err(
                        c,
                        _("parent-directory manifest refers to unknown " "revision %s")
                        % short(m),
                        label,
                    )
                else:
                    self.err(
                        c,
                        _("changeset refers to unknown revision %s") % short(m),
                        label,
                    )

        return filenodes, subdirnodes

    def _verifymanifesttree(self, filenodes, subdirnodes, storefiles, progress):
        for subdir, linkrevs in subdirnodes.iteritems():
            progress.value += 1
            subdirfilenodes, subsubdirnodes = self._verifymanifestpart(
                linkrevs, subdir, storefiles, progress
            )
            for f, onefilenodes in subdirfilenodes.iteritems():
                filenodes.setdefault(f, {}).update(onefilenodes)
            self._verifymanifesttree(filenodes, subsubdirnodes, storefiles, progress)

    def _crosscheckfiles(self, filelinkrevs, filenodes):
        if self.ui.configbool("verify", "skipmanifests", False):
            return

        repo = self.repo
        ui = self.ui
        ui.status(_("crosschecking files in changesets and manifests\n"))

        total = len(filelinkrevs) + len(filenodes)
        with progress.bar(ui, _("crosschecking"), total=total) as prog:
            if self.havemf and self.revs is None:
                # only check whether changed files from changesets exist
                # in manifests when verifying the entire repo
                for f in sorted(filelinkrevs):
                    prog.value += 1
                    if f not in filenodes:
                        lr = filelinkrevs[f][0]
                        self.err(lr, _("in changeset but not in manifest"), f)

            if self.havecl:
                for f in sorted(filenodes):
                    prog.value += 1
                    if f not in filelinkrevs:
                        try:
                            fl = repo.file(f)
                            lr = min([fl.linkrev(fl.rev(n)) for n in filenodes[f]])
                        except Exception:
                            lr = None
                        self.err(lr, _("in manifest but not in changeset"), f)

    def _verifyfiles(self, filenodes, filelinkrevs):
        if self.ui.configbool("verify", "skipmanifests", False):
            return 0, 0

        repo = self.repo
        ui = self.ui
        revlogv1 = self.revlogv1
        ui.status(_("checking files\n"))

        storefiles = set()
        if self.revs is None:
            # only check store files when verifying the entire repo
            for f, f2, size in repo.store.datafiles():
                if not f:
                    self.err(None, _("cannot decode filename '%s'") % f2)
                elif (size > 0 or not revlogv1) and f.startswith("data/"):
                    storefiles.add(_normpath(f))
        files = sorted(set(filenodes) | set(filelinkrevs))
        total = len(files)
        with progress.bar(ui, _("checking"), _("files"), total) as prog:
            revisions = self._verifyfilelist(
                filenodes, files, filelinkrevs, storefiles, prog
            )

        if self.revs is None:
            for f in sorted(storefiles):
                self.warn(_("warning: orphan revlog '%s'") % f)

        return len(files), revisions

    def _verifyfilelist(self, filenodes, files, filelinkrevs, storefiles, prog):
        repo = self.repo
        ui = self.ui
        lrugetctx = self.lrugetctx
        havemf = self.havemf
        revisions = 0
        for i, f in enumerate(files):
            prog.value = (i, f)
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

            if self.revs is None:
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
                if self.revs is not None and fl.linkrev(i) not in self.revs:
                    continue
                revisions += 1
                n = fl.node(i)
                lr = self.checkentry(fl, i, n, seen, linkrevs, f)
                if f in filenodes:
                    if havemf and n not in filenodes[f]:
                        self.err(lr, _("%s not in manifests") % (short(n)), f)
                    else:
                        del filenodes[f][n]

                # Verify contents. 4 cases to care about:
                #
                #   common: the most common case
                #   rename: with a rename
                #   meta: file content starts with b'\1\n', the metadata
                #         header defined in filelog.py, but without a rename
                #   ext: content stored externally
                #
                # More formally, their differences are shown below:
                #
                #                       | common | rename | meta  | ext
                #  -------------------------------------------------------
                #   flags()             | 0      | 0      | 0     | not 0
                #   renamed()           | False  | True   | False | ?
                #   rawtext[0:2]=='\1\n'| False  | True   | True  | ?
                #
                # "rawtext" means the raw text stored in revlog data, which
                # could be retrieved by "revision(rev, raw=True)". "text"
                # mentioned below is "revision(rev, raw=False)".
                #
                # There are 3 different lengths stored physically:
                #  1. L1: rawsize, stored in revlog index
                #  2. L2: len(rawtext), stored in revlog data
                #  3. L3: len(text), stored in revlog data if flags==0, or
                #     possibly somewhere else if flags!=0
                #
                # L1 should be equal to L2. L3 could be different from them.
                # "text" may or may not affect commit hash depending on flag
                # processors (see revlog.addflagprocessor).
                #
                #              | common  | rename | meta  | ext
                # -------------------------------------------------
                #    rawsize() | L1      | L1     | L1    | L1
                #       size() | L1      | L2-LM  | L1(*) | L1 (?)
                # len(rawtext) | L2      | L2     | L2    | L2
                #    len(text) | L2      | L2     | L2    | L3
                #  len(read()) | L2      | L2-LM  | L2-LM | L3 (?)
                #
                # LM:  length of metadata, depending on rawtext
                # (*): not ideal, see comment in filelog.size
                # (?): could be "- len(meta)" if the resolved content has
                #      rename metadata
                #
                # Checks needed to be done:
                #  1. length check: L1 == L2, in all cases.
                #  2. hash check: depending on flag processor, we may need to
                #     use either "text" (external), or "rawtext" (in revlog).
                try:
                    skipflags = self.skipflags
                    if skipflags:
                        skipflags &= fl.flags(i)
                    if not skipflags:
                        fl.read(n)  # side effect: read content and do checkhash
                        rp = fl.renamed(n)
                    # the "L1 == L2" check
                    l1 = fl.rawsize(i)
                    l2 = len(fl.revision(n, raw=True))
                    if l1 != l2:
                        self.err(
                            lr, _("unpacked size is %s, %s expected") % (l2, l1), f
                        )
                except error.CensoredNodeError:
                    # experimental config: censor.policy
                    if ui.config("censor", "policy") == "abort":
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
                                self.warn(
                                    _(
                                        "warning: copy source of '%s' not"
                                        " in parents of %s"
                                    )
                                    % (f, ctx)
                                )
                        fl2 = repo.file(rp[0])
                        if not len(fl2):
                            self.err(
                                lr,
                                _("empty or missing copy source " "revlog %s:%s")
                                % (rp[0], short(rp[1])),
                                f,
                            )
                        elif rp[1] == nullid:
                            ui.note(
                                _(
                                    "warning: %s@%s: copy source"
                                    " revision is nullid %s:%s\n"
                                )
                                % (f, lr, rp[0], short(rp[1]))
                            )
                        else:
                            fl2.rev(rp[1])
                except Exception as inst:
                    self.exc(lr, _("checking rename of %s") % short(n), inst, f)

            # cross-check
            if f in filenodes:
                fns = [(v, k) for k, v in filenodes[f].iteritems()]
                for lr, node in sorted(fns):
                    self.err(
                        lr, _("manifest refers to unknown revision %s") % short(node), f
                    )

        return revisions
