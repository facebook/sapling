# verify.py - repository integrity checking for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from node import nullid, short
from i18n import _
import revlog, util, error

def verify(repo):
    lock = repo.lock()
    try:
        return _verify(repo)
    finally:
        lock.release()

def _verify(repo):
    mflinkrevs = {}
    filelinkrevs = {}
    filenodes = {}
    revisions = 0
    badrevs = set()
    errors = [0]
    warnings = [0]
    ui = repo.ui
    cl = repo.changelog
    mf = repo.manifest

    if not repo.cancopy():
        raise util.Abort(_("cannot verify bundle or remote repos"))

    def err(linkrev, msg, filename=None):
        if linkrev != None:
            badrevs.add(linkrev)
        else:
            linkrev = '?'
        msg = "%s: %s" % (linkrev, msg)
        if filename:
            msg = "%s@%s" % (filename, msg)
        ui.warn(" " + msg + "\n")
        errors[0] += 1

    def exc(linkrev, msg, inst, filename=None):
        if isinstance(inst, KeyboardInterrupt):
            ui.warn(_("interrupted"))
            raise
        err(linkrev, "%s: %s" % (msg, inst), filename)

    def warn(msg):
        ui.warn(msg + "\n")
        warnings[0] += 1

    def checklog(obj, name, linkrev):
        if not len(obj) and (havecl or havemf):
            err(linkrev, _("empty or missing %s") % name)
            return

        d = obj.checksize()
        if d[0]:
            err(None, _("data length off by %d bytes") % d[0], name)
        if d[1]:
            err(None, _("index contains %d extra bytes") % d[1], name)

        if obj.version != revlog.REVLOGV0:
            if not revlogv1:
                warn(_("warning: `%s' uses revlog format 1") % name)
        elif revlogv1:
            warn(_("warning: `%s' uses revlog format 0") % name)

    def checkentry(obj, i, node, seen, linkrevs, f):
        lr = obj.linkrev(obj.rev(node))
        if lr < 0 or (havecl and lr not in linkrevs):
            if lr < 0 or lr >= len(cl):
                msg = _("rev %d points to nonexistent changeset %d")
            else:
                msg = _("rev %d points to unexpected changeset %d")
            err(None, msg % (i, lr), f)
            if linkrevs:
                warn(_(" (expected %s)") % " ".join(map(str, linkrevs)))
            lr = None # can't be trusted

        try:
            p1, p2 = obj.parents(node)
            if p1 not in seen and p1 != nullid:
                err(lr, _("unknown parent 1 %s of %s") %
                    (short(p1), short(n)), f)
            if p2 not in seen and p2 != nullid:
                err(lr, _("unknown parent 2 %s of %s") %
                    (short(p2), short(p1)), f)
        except Exception, inst:
            exc(lr, _("checking parents of %s") % short(node), inst, f)

        if node in seen:
            err(lr, _("duplicate revision %d (%d)") % (i, seen[n]), f)
        seen[n] = i
        return lr

    revlogv1 = cl.version != revlog.REVLOGV0
    if ui.verbose or not revlogv1:
        ui.status(_("repository uses revlog format %d\n") %
                       (revlogv1 and 1 or 0))

    havecl = len(cl) > 0
    havemf = len(mf) > 0

    ui.status(_("checking changesets\n"))
    seen = {}
    checklog(cl, "changelog", 0)
    for i in repo:
        n = cl.node(i)
        checkentry(cl, i, n, seen, [i], "changelog")

        try:
            changes = cl.read(n)
            mflinkrevs.setdefault(changes[0], []).append(i)
            for f in changes[3]:
                filelinkrevs.setdefault(f, []).append(i)
        except Exception, inst:
            exc(i, _("unpacking changeset %s") % short(n), inst)

    ui.status(_("checking manifests\n"))
    seen = {}
    checklog(mf, "manifest", 0)
    for i in mf:
        n = mf.node(i)
        lr = checkentry(mf, i, n, seen, mflinkrevs.get(n, []), "manifest")
        if n in mflinkrevs:
            del mflinkrevs[n]
        else:
            err(lr, _("%s not in changesets") % short(n), "manifest")

        try:
            for f, fn in mf.readdelta(n).iteritems():
                if not f:
                    err(lr, _("file without name in manifest"))
                elif f != "/dev/null":
                    fns = filenodes.setdefault(f, {})
                    if fn not in fns:
                        fns[fn] = i
        except Exception, inst:
            exc(lr, _("reading manifest delta %s") % short(n), inst)

    ui.status(_("crosschecking files in changesets and manifests\n"))

    if havemf:
        for c,m in sorted([(c, m) for m in mflinkrevs for c in mflinkrevs[m]]):
            err(c, _("changeset refers to unknown manifest %s") % short(m))
        mflinkrevs = None # del is bad here due to scope issues

        for f in sorted(filelinkrevs):
            if f not in filenodes:
                lr = filelinkrevs[f][0]
                err(lr, _("in changeset but not in manifest"), f)

    if havecl:
        for f in sorted(filenodes):
            if f not in filelinkrevs:
                try:
                    fl = repo.file(f)
                    lr = min([fl.linkrev(fl.rev(n)) for n in filenodes[f]])
                except:
                    lr = None
                err(lr, _("in manifest but not in changeset"), f)

    ui.status(_("checking files\n"))

    storefiles = set()
    for f, f2, size in repo.store.datafiles():
        if not f:
            err(None, _("cannot decode filename '%s'") % f2)
        elif size > 0:
            storefiles.add(f)

    files = sorted(set(filenodes) | set(filelinkrevs))
    for f in files:
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
        except error.RevlogError, e:
            err(lr, _("broken revlog! (%s)") % e, f)
            continue

        for ff in fl.files():
            try:
                storefiles.remove(ff)
            except KeyError:
                err(lr, _("missing revlog!"), ff)

        checklog(fl, f, lr)
        seen = {}
        for i in fl:
            revisions += 1
            n = fl.node(i)
            lr = checkentry(fl, i, n, seen, linkrevs, f)
            if f in filenodes:
                if havemf and n not in filenodes[f]:
                    err(lr, _("%s not in manifests") % (short(n)), f)
                else:
                    del filenodes[f][n]

            # verify contents
            try:
                t = fl.read(n)
                rp = fl.renamed(n)
                if len(t) != fl.size(i):
                    if len(fl.revision(n)) != fl.size(i):
                        err(lr, _("unpacked size is %s, %s expected") %
                            (len(t), fl.size(i)), f)
            except Exception, inst:
                exc(lr, _("unpacking %s") % short(n), inst, f)

            # check renames
            try:
                if rp:
                    fl2 = repo.file(rp[0])
                    if not len(fl2):
                        err(lr, _("empty or missing copy source revlog %s:%s")
                            % (rp[0], short(rp[1])), f)
                    elif rp[1] == nullid:
                        ui.note(_("warning: %s@%s: copy source"
                                  " revision is nullid %s:%s\n")
                            % (f, lr, rp[0], short(rp[1])))
                    else:
                        fl2.rev(rp[1])
            except Exception, inst:
                exc(lr, _("checking rename of %s") % short(n), inst, f)

        # cross-check
        if f in filenodes:
            fns = [(mf.linkrev(l), n) for n,l in filenodes[f].iteritems()]
            for lr, node in sorted(fns):
                err(lr, _("%s in manifests not found") % short(node), f)

    for f in storefiles:
        warn(_("warning: orphan revlog '%s'") % f)

    ui.status(_("%d files, %d changesets, %d total revisions\n") %
                   (len(files), len(cl), revisions))
    if warnings[0]:
        ui.warn(_("%d warnings encountered!\n") % warnings[0])
    if errors[0]:
        ui.warn(_("%d integrity errors encountered!\n") % errors[0])
        if badrevs:
            ui.warn(_("(first damaged changeset appears to be %d)\n")
                    % min(badrevs))
        return 1
