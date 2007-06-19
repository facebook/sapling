# verify.py - repository integrity checking for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from i18n import _
import revlog, mdiff

def verify(repo):
    filelinkrevs = {}
    filenodes = {}
    changesets = revisions = files = 0
    errors = [0]
    warnings = [0]
    neededmanifests = {}

    lock = repo.lock()

    def err(msg):
        repo.ui.warn(msg + "\n")
        errors[0] += 1

    def warn(msg):
        repo.ui.warn(msg + "\n")
        warnings[0] += 1

    def checksize(obj, name):
        d = obj.checksize()
        if d[0]:
            err(_("%s data length off by %d bytes") % (name, d[0]))
        if d[1]:
            err(_("%s index contains %d extra bytes") % (name, d[1]))

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

    seen = {}
    repo.ui.status(_("checking changesets\n"))
    checksize(repo.changelog, "changelog")

    for i in xrange(repo.changelog.count()):
        changesets += 1
        n = repo.changelog.node(i)
        l = repo.changelog.linkrev(n)
        if l != i:
            err(_("incorrect link (%d) for changeset revision %d") %(l, i))
        if n in seen:
            err(_("duplicate changeset at revision %d") % i)
        seen[n] = 1

        for p in repo.changelog.parents(n):
            if p not in repo.changelog.nodemap:
                err(_("changeset %s has unknown parent %s") %
                             (short(n), short(p)))
        try:
            changes = repo.changelog.read(n)
        except KeyboardInterrupt:
            repo.ui.warn(_("interrupted"))
            raise
        except Exception, inst:
            err(_("unpacking changeset %s: %s") % (short(n), inst))
            continue

        neededmanifests[changes[0]] = n

        for f in changes[3]:
            filelinkrevs.setdefault(f, []).append(i)

    seen = {}
    repo.ui.status(_("checking manifests\n"))
    checkversion(repo.manifest, "manifest")
    checksize(repo.manifest, "manifest")

    for i in xrange(repo.manifest.count()):
        n = repo.manifest.node(i)
        l = repo.manifest.linkrev(n)

        if l < 0 or l >= repo.changelog.count():
            err(_("bad manifest link (%d) at revision %d") % (l, i))

        if n in neededmanifests:
            del neededmanifests[n]

        if n in seen:
            err(_("duplicate manifest at revision %d") % i)

        seen[n] = 1

        for p in repo.manifest.parents(n):
            if p not in repo.manifest.nodemap:
                err(_("manifest %s has unknown parent %s") %
                    (short(n), short(p)))

        try:
            for f, fn in repo.manifest.readdelta(n).iteritems():
                filenodes.setdefault(f, {})[fn] = 1
        except KeyboardInterrupt:
            repo.ui.warn(_("interrupted"))
            raise
        except Exception, inst:
            err(_("reading delta for manifest %s: %s") % (short(n), inst))
            continue

    repo.ui.status(_("crosschecking files in changesets and manifests\n"))

    for m, c in neededmanifests.items():
        err(_("Changeset %s refers to unknown manifest %s") %
            (short(m), short(c)))
    del neededmanifests

    for f in filenodes:
        if f not in filelinkrevs:
            err(_("file %s in manifest but not in changesets") % f)

    for f in filelinkrevs:
        if f not in filenodes:
            err(_("file %s in changeset but not in manifest") % f)

    repo.ui.status(_("checking files\n"))
    ff = filenodes.keys()
    ff.sort()
    for f in ff:
        if f == "/dev/null":
            continue
        files += 1
        if not f:
            err(_("file without name in manifest %s") % short(n))
            continue
        fl = repo.file(f)
        checkversion(fl, f)
        checksize(fl, f)

        nodes = {nullid: 1}
        seen = {}
        for i in xrange(fl.count()):
            revisions += 1
            n = fl.node(i)

            if n in seen:
                err(_("%s: duplicate revision %d") % (f, i))
            if n not in filenodes[f]:
                err(_("%s: %d:%s not in manifests") % (f, i, short(n)))
            else:
                del filenodes[f][n]

            flr = fl.linkrev(n)
            if flr not in filelinkrevs.get(f, []):
                err(_("%s:%s points to unexpected changeset %d")
                        % (f, short(n), flr))
            else:
                filelinkrevs[f].remove(flr)

            # verify contents
            try:
                t = fl.read(n)
            except KeyboardInterrupt:
                repo.ui.warn(_("interrupted"))
                raise
            except Exception, inst:
                err(_("unpacking file %s %s: %s") % (f, short(n), inst))

            # verify parents
            (p1, p2) = fl.parents(n)
            if p1 not in nodes:
                err(_("file %s:%s unknown parent 1 %s") %
                    (f, short(n), short(p1)))
            if p2 not in nodes:
                err(_("file %s:%s unknown parent 2 %s") %
                        (f, short(n), short(p1)))
            nodes[n] = 1

            # check renames
            try:
                rp = fl.renamed(n)
                if rp:
                    fl2 = repo.file(rp[0])
                    rev = fl2.rev(rp[1])
            except KeyboardInterrupt:
                repo.ui.warn(_("interrupted"))
                raise
            except Exception, inst:
                err(_("checking rename on file %s %s: %s") % (f, short(n), inst))

        # cross-check
        for node in filenodes[f]:
            err(_("node %s in manifests not in %s") % (hex(node), f))

    repo.ui.status(_("%d files, %d changesets, %d total revisions\n") %
                   (files, changesets, revisions))

    if warnings[0]:
        repo.ui.warn(_("%d warnings encountered!\n") % warnings[0])
    if errors[0]:
        repo.ui.warn(_("%d integrity errors encountered!\n") % errors[0])
        return 1

