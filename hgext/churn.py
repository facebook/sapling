# churn.py - create a graph showing who changed the most lines
#
# Copyright 2006 Josef "Jeff" Sipek <jeffpc@josefsipek.net>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
#
# Aliases map file format is simple one alias per line in the following
# format:
#
# <alias email> <actual email>

from mercurial.i18n import gettext as _
from mercurial import mdiff, cmdutil, util, node
import os, sys

def get_tty_width():
    if 'COLUMNS' in os.environ:
        try:
            return int(os.environ['COLUMNS'])
        except ValueError:
            pass
    try:
        import termios, array, fcntl
        for dev in (sys.stdout, sys.stdin):
            try:
                fd = dev.fileno()
                if not os.isatty(fd):
                    continue
                arri = fcntl.ioctl(fd, termios.TIOCGWINSZ, '\0' * 8)
                return array.array('h', arri)[1]
            except ValueError:
                pass
    except ImportError:
        pass
    return 80

def __gather(ui, repo, node1, node2):
    def dirtywork(f, mmap1, mmap2):
        lines = 0

        to = mmap1 and repo.file(f).read(mmap1[f]) or None
        tn = mmap2 and repo.file(f).read(mmap2[f]) or None

        diff = mdiff.unidiff(to, "", tn, "", f, f).split("\n")

        for line in diff:
            if not line:
                continue # skip EOF
            if line.startswith(" "):
                continue # context line
            if line.startswith("--- ") or line.startswith("+++ "):
                continue # begining of diff
            if line.startswith("@@ "):
                continue # info line

            # changed lines
            lines += 1

        return lines

    ##

    lines = 0

    changes = repo.status(node1, node2, None, util.always)[:5]

    modified, added, removed, deleted, unknown = changes

    who = repo.changelog.read(node2)[1]
    who = util.email(who) # get the email of the person

    mmap1 = repo.manifest.read(repo.changelog.read(node1)[0])
    mmap2 = repo.manifest.read(repo.changelog.read(node2)[0])
    for f in modified:
        lines += dirtywork(f, mmap1, mmap2)

    for f in added:
        lines += dirtywork(f, None, mmap2)

    for f in removed:
        lines += dirtywork(f, mmap1, None)

    for f in deleted:
        lines += dirtywork(f, mmap1, mmap2)

    for f in unknown:
        lines += dirtywork(f, mmap1, mmap2)

    return (who, lines)

def gather_stats(ui, repo, amap, revs=None, progress=False):
    stats = {}

    cl    = repo.changelog

    if not revs:
        revs = range(0, cl.count())

    nr_revs = len(revs)
    cur_rev = 0

    for rev in revs:
        cur_rev += 1 # next revision

        node2    = cl.node(rev)
        node1    = cl.parents(node2)[0]

        if cl.parents(node2)[1] != node.nullid:
            ui.note(_('Revision %d is a merge, ignoring...\n') % (rev,))
            continue

        who, lines = __gather(ui, repo, node1, node2)

        # remap the owner if possible
        if who in amap:
            ui.note("using '%s' alias for '%s'\n" % (amap[who], who))
            who = amap[who]

        if not who in stats:
            stats[who] = 0
        stats[who] += lines

        ui.note("rev %d: %d lines by %s\n" % (rev, lines, who))

        if progress:
            nr_revs = max(nr_revs, 1)
            if int(100.0*(cur_rev - 1)/nr_revs) < int(100.0*cur_rev/nr_revs):
                ui.write("\rGenerating stats: %d%%" % (int(100.0*cur_rev/nr_revs),))
                sys.stdout.flush()

    if progress:
        ui.write("\r")
        sys.stdout.flush()

    return stats

def churn(ui, repo, **opts):
    "Graphs the number of lines changed"

    def pad(s, l):
        if len(s) < l:
            return s + " " * (l-len(s))
        return s[0:l]

    def graph(n, maximum, width, char):
        maximum = max(1, maximum)
        n = int(n * width / float(maximum))

        return char * (n)

    def get_aliases(f):
        aliases = {}

        for l in f.readlines():
            l = l.strip()
            alias, actual = l.split()
            aliases[alias] = actual

        return aliases

    amap = {}
    aliases = opts.get('aliases')
    if aliases:
        try:
            f = open(aliases,"r")
        except OSError, e:
            print "Error: " + e
            return

        amap = get_aliases(f)
        f.close()

    revs = [int(r) for r in cmdutil.revrange(repo, opts['rev'])]
    revs.sort()
    stats = gather_stats(ui, repo, amap, revs, opts.get('progress'))

    # make a list of tuples (name, lines) and sort it in descending order
    ordered = stats.items()
    if not ordered:
        return
    ordered.sort(lambda x, y: cmp(y[1], x[1]))
    max_churn = ordered[0][1]

    tty_width = get_tty_width()
    ui.note(_("assuming %i character terminal\n") % tty_width)
    tty_width -= 1

    max_user_width = max([len(user) for user, churn in ordered])

    graph_width = tty_width - max_user_width - 1 - 6 - 2 - 2

    for user, churn in ordered:
        print "%s %6d %s" % (pad(user, max_user_width),
                             churn,
                             graph(churn, max_churn, graph_width, '*'))

cmdtable = {
    "churn":
    (churn,
     [('r', 'rev', [], _('limit statistics to the specified revisions')),
      ('', 'aliases', '', _('file with email aliases')),
      ('', 'progress', None, _('show progress'))],
    'hg churn [-r revision range] [-a file] [--progress]'),
}
