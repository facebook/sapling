# repair.py - functions for repository repair for mercurial
#
# Copyright 2005, 2006 Chris Mason <mason@suse.com>
# Copyright 2007 Matt Mackall
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import changegroup, bookmarks
from node import nullrev, short
from i18n import _
import os

def _bundle(repo, bases, heads, node, suffix, extranodes=None, compress=True):
    """create a bundle with the specified revisions as a backup"""
    cg = repo.changegroupsubset(bases, heads, 'strip', extranodes)
    backupdir = repo.join("strip-backup")
    if not os.path.isdir(backupdir):
        os.mkdir(backupdir)
    name = os.path.join(backupdir, "%s-%s.hg" % (short(node), suffix))
    if compress:
        bundletype = "HG10BZ"
    else:
        bundletype = "HG10UN"
    return changegroup.writebundle(cg, name, bundletype)

def _collectfiles(repo, striprev):
    """find out the filelogs affected by the strip"""
    files = set()

    for x in xrange(striprev, len(repo)):
        files.update(repo[x].files())

    return sorted(files)

def _collectextranodes(repo, files, link):
    """return the nodes that have to be saved before the strip"""
    def collectone(cl, revlog):
        extra = []
        startrev = count = len(revlog)
        # find the truncation point of the revlog
        for i in xrange(count):
            lrev = revlog.linkrev(i)
            if lrev >= link:
                startrev = i + 1
                break

        # see if any revision after that point has a linkrev less than link
        # (we have to manually save these guys)
        for i in xrange(startrev, count):
            node = revlog.node(i)
            lrev = revlog.linkrev(i)
            if lrev < link:
                extra.append((node, cl.node(lrev)))

        return extra

    extranodes = {}
    cl = repo.changelog
    extra = collectone(cl, repo.manifest)
    if extra:
        extranodes[1] = extra
    for fname in files:
        f = repo.file(fname)
        extra = collectone(cl, f)
        if extra:
            extranodes[fname] = extra

    return extranodes

def strip(ui, repo, node, backup="all"):
    cl = repo.changelog
    # TODO delete the undo files, and handle undo of merge sets
    striprev = cl.rev(node)

    keeppartialbundle = backup == 'strip'

    # Some revisions with rev > striprev may not be descendants of striprev.
    # We have to find these revisions and put them in a bundle, so that
    # we can restore them after the truncations.
    # To create the bundle we use repo.changegroupsubset which requires
    # the list of heads and bases of the set of interesting revisions.
    # (head = revision in the set that has no descendant in the set;
    #  base = revision in the set that has no ancestor in the set)
    tostrip = set((striprev,))
    saveheads = set()
    savebases = []
    for r in xrange(striprev + 1, len(cl)):
        parents = cl.parentrevs(r)
        if parents[0] in tostrip or parents[1] in tostrip:
            # r is a descendant of striprev
            tostrip.add(r)
            # if this is a merge and one of the parents does not descend
            # from striprev, mark that parent as a savehead.
            if parents[1] != nullrev:
                for p in parents:
                    if p not in tostrip and p > striprev:
                        saveheads.add(p)
        else:
            # if no parents of this revision will be stripped, mark it as
            # a savebase
            if parents[0] < striprev and parents[1] < striprev:
                savebases.append(cl.node(r))

            saveheads.difference_update(parents)
            saveheads.add(r)

    bm = repo._bookmarks
    updatebm = []
    for m in bm:
        rev = repo[bm[m]].rev()
        if rev in tostrip:
            updatebm.append(m)

    saveheads = [cl.node(r) for r in saveheads]
    files = _collectfiles(repo, striprev)

    extranodes = _collectextranodes(repo, files, striprev)

    # create a changegroup for all the branches we need to keep
    backupfile = None
    if backup == "all":
        backupfile = _bundle(repo, [node], cl.heads(), node, 'backup')
        repo.ui.status(_("saved backup bundle to %s\n") % backupfile)
    if saveheads or extranodes:
        # do not compress partial bundle if we remove it from disk later
        chgrpfile = _bundle(repo, savebases, saveheads, node, 'temp',
                            extranodes=extranodes, compress=keeppartialbundle)

    mfst = repo.manifest

    tr = repo.transaction("strip")
    offset = len(tr.entries)

    try:
        tr.startgroup()
        cl.strip(striprev, tr)
        mfst.strip(striprev, tr)
        for fn in files:
            repo.file(fn).strip(striprev, tr)
        tr.endgroup()

        try:
            for i in xrange(offset, len(tr.entries)):
                file, troffset, ignore = tr.entries[i]
                repo.sopener(file, 'a').truncate(troffset)
            tr.close()
        except:
            tr.abort()
            raise

        if saveheads or extranodes:
            ui.note(_("adding branch\n"))
            f = open(chgrpfile, "rb")
            gen = changegroup.readbundle(f, chgrpfile)
            if not repo.ui.verbose:
                # silence internal shuffling chatter
                repo.ui.pushbuffer()
            repo.addchangegroup(gen, 'strip', 'bundle:' + chgrpfile, True)
            if not repo.ui.verbose:
                repo.ui.popbuffer()
            f.close()
            if not keeppartialbundle:
                os.unlink(chgrpfile)

        for m in updatebm:
            bm[m] = repo['.'].node()
        bookmarks.write(repo)

    except:
        if backupfile:
            ui.warn(_("strip failed, full bundle stored in '%s'\n")
                    % backupfile)
        elif saveheads:
            ui.warn(_("strip failed, partial bundle stored in '%s'\n")
                    % chgrpfile)
        raise

    repo.destroyed()
