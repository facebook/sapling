# verify.py - repository integrity checking for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import nullid, short
from i18n import _
import revlog

def verify(repo):
    lock = repo.lock()
    try:
        return _verify(repo)
    finally:
        del lock

def _verify(repo):
    filelinkrevs = {}
    filenodes = {}
    changesets = revisions = files = 0
    firstbad = [None]
    errors = [0]
    warnings = [0]
    neededmanifests = {}

    def err(linkrev, msg, filename=None):
        if linkrev != None:
            if firstbad[0] != None:
                firstbad[0] = min(firstbad[0], linkrev)
            else:
                firstbad[0] = linkrev
        else:
            linkrev = "?"
        msg = "%s: %s" % (linkrev, msg)
        if filename:
            msg = "%s@%s" % (filename, msg)
        repo.ui.warn(" " + msg + "\n")
        errors[0] += 1

    def warn(msg):
        repo.ui.warn(msg + "\n")
        warnings[0] += 1

    def checksize(obj, name):
        d = obj.checksize()
        if d[0]:
            err(None, _("data length off by %d bytes") % d[0], name)
        if d[1]:
            err(None, _("index contains %d extra bytes") % d[1], name)

    def checkversion(obj, name):
        if obj.version != revlog.REVLOGV0:
            if not revlogv1:
                warn(_("warning: `%s' uses revlog format 1") % name)
        elif revlogv1:
            warn(_("warning: `%s' uses revlog format 0") % name)

    revlogv1 = repo.changelog.version != revlog.REVLOGV0
    if repo.ui.verbose or not revlogv1:
        repo.ui.status(_("repository uses revlog format %d\n") %
                       (revlogv1 and 1 or 0))

    havecl = havemf = 1
    seen = {}
    repo.ui.status(_("checking changesets\n"))
    if repo.changelog.count() == 0 and repo.manifest.count() > 1:
        havecl = 0
        err(0, _("empty or missing 00changelog.i"))
    else:
        checksize(repo.changelog, "changelog")

    for i in xrange(repo.changelog.count()):
        changesets += 1
        n = repo.changelog.node(i)
        l = repo.changelog.linkrev(n)
        if l != i:
            err(i, _("incorrect link (%d) for changeset") %(l))
        if n in seen:
            err(i, _("duplicates changeset at revision %d") % seen[n])
        seen[n] = i

        for p in repo.changelog.parents(n):
            if p not in repo.changelog.nodemap:
                err(i, _("changeset has unknown parent %s") % short(p))
        try:
            changes = repo.changelog.read(n)
        except KeyboardInterrupt:
            repo.ui.warn(_("interrupted"))
            raise
        except Exception, inst:
            err(i, _("unpacking changeset: %s") % inst)
            continue

        if changes[0] not in neededmanifests:
            neededmanifests[changes[0]] = i

        for f in changes[3]:
            filelinkrevs.setdefault(f, []).append(i)

    seen = {}
    repo.ui.status(_("checking manifests\n"))
    if repo.changelog.count() > 0 and repo.manifest.count() == 0:
        havemf = 0
        err(0, _("empty or missing 00manifest.i"))
    else:
        checkversion(repo.manifest, "manifest")
        checksize(repo.manifest, "manifest")

    for i in xrange(repo.manifest.count()):
        n = repo.manifest.node(i)
        l = repo.manifest.linkrev(n)

        if l < 0 or (havecl and l >= repo.changelog.count()):
            err(None, _("bad link (%d) at manifest revision %d") % (l, i))

        if n in neededmanifests:
            del neededmanifests[n]

        if n in seen:
            err(l, _("duplicates manifest from %d") % seen[n])

        seen[n] = l

        for p in repo.manifest.parents(n):
            if p not in repo.manifest.nodemap:
                err(l, _("manifest has unknown parent %s") % short(p))

        try:
            for f, fn in repo.manifest.readdelta(n).iteritems():
                fns = filenodes.setdefault(f, {})
                if fn not in fns:
                    fns[fn] = n
        except KeyboardInterrupt:
            repo.ui.warn(_("interrupted"))
            raise
        except Exception, inst:
            err(l, _("reading manifest delta: %s") % inst)
            continue

    repo.ui.status(_("crosschecking files in changesets and manifests\n"))

    if havemf > 0:
        nm = [(c, m) for m, c in neededmanifests.items()]
        nm.sort()
        for c, m in nm:
            err(c, _("changeset refers to unknown manifest %s") % short(m))
        del neededmanifests, nm

    if havecl:
        fl = filenodes.keys()
        fl.sort()
        for f in fl:
            if f not in filelinkrevs:
                lrs = [repo.manifest.linkrev(n) for n in filenodes[f]]
                lrs.sort()
                err(lrs[0], _("in manifest but not in changeset"), f)
        del fl

    if havemf:
        fl = filelinkrevs.keys()
        fl.sort()
        for f in fl:
            if f not in filenodes:
                lr = filelinkrevs[f][0]
                err(lr, _("in changeset but not in manifest"), f)
        del fl

    repo.ui.status(_("checking files\n"))
    ff = dict.fromkeys(filenodes.keys() + filelinkrevs.keys()).keys()
    ff.sort()
    for f in ff:
        if f == "/dev/null":
            continue
        files += 1
        if not f:
            lr = filelinkrevs[f][0]
            err(lr, _("file without name in manifest"))
            continue
        fl = repo.file(f)
        checkversion(fl, f)
        checksize(fl, f)

        if fl.count() == 0:
            err(filelinkrevs[f][0], _("empty or missing revlog"), f)
            continue

        seen = {}
        nodes = {nullid: 1}
        for i in xrange(fl.count()):
            revisions += 1
            n = fl.node(i)
            flr = fl.linkrev(n)

            if flr < 0 or (havecl and flr not in filelinkrevs.get(f, [])):
                if flr < 0 or flr >= repo.changelog.count():
                    err(None, _("rev %d point to nonexistent changeset %d")
                        % (i, flr), f)
                else:
                    err(None, _("rev %d points to unexpected changeset %d")
                        % (i, flr), f)
                if f in filelinkrevs:
                    warn(_(" (expected %s)") % filelinkrevs[f][0])
                flr = None # can't be trusted
            else:
                if havecl:
                    filelinkrevs[f].remove(flr)

            if n in seen:
                err(flr, _("duplicate revision %d") % i, f)
            if f in filenodes:
                if havemf and n not in filenodes[f]:
                    err(flr, _("%s not in manifests") % (short(n)), f)
                else:
                    del filenodes[f][n]

            # verify contents
            try:
                t = fl.read(n)
            except KeyboardInterrupt:
                repo.ui.warn(_("interrupted"))
                raise
            except Exception, inst:
                err(flr, _("unpacking %s: %s") % (short(n), inst), f)

            # verify parents
            try:
                (p1, p2) = fl.parents(n)
                if p1 not in nodes:
                    err(flr, _("unknown parent 1 %s of %s") %
                        (short(p1), short(n)), f)
                if p2 not in nodes:
                    err(flr, _("unknown parent 2 %s of %s") %
                            (short(p2), short(p1)), f)
            except KeyboardInterrupt:
                repo.ui.warn(_("interrupted"))
                raise
            except Exception, inst:
                err(flr, _("checking parents of %s: %s") % (short(n), inst), f)
            nodes[n] = 1

            # check renames
            try:
                rp = fl.renamed(n)
                if rp:
                    fl2 = repo.file(rp[0])
                    if fl2.count() == 0:
                        err(flr, _("empty or missing copy source revlog %s:%s")
                            % (rp[0], short(rp[1])), f)
                    elif rp[1] == nullid:
                        err(flr, _("copy source revision is nullid %s:%s")
                            % (rp[0], short(rp[1])), f)
                    else:
                        rev = fl2.rev(rp[1])
            except KeyboardInterrupt:
                repo.ui.warn(_("interrupted"))
                raise
            except Exception, inst:
                err(flr, _("checking rename of %s: %s") %
                    (short(n), inst), f)

        # cross-check
        if f in filenodes:
            fns = [(repo.manifest.linkrev(filenodes[f][n]), n)
                   for n in filenodes[f]]
            fns.sort()
            for lr, node in fns:
                err(lr, _("%s in manifests not found") % short(node), f)

    repo.ui.status(_("%d files, %d changesets, %d total revisions\n") %
                   (files, changesets, revisions))

    if warnings[0]:
        repo.ui.warn(_("%d warnings encountered!\n") % warnings[0])
    if errors[0]:
        repo.ui.warn(_("%d integrity errors encountered!\n") % errors[0])
        if firstbad[0]:
            repo.ui.warn(_("(first damaged changeset appears to be %d)\n")
                         % firstbad[0])
        return 1
