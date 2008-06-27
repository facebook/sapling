# churn.py - create a graph showing who changed the most lines
#
# Copyright 2006 Josef "Jeff" Sipek <jeffpc@josefsipek.net>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
'''allow graphing the number of lines changed per contributor'''

from mercurial.i18n import gettext as _
from mercurial import patch, cmdutil, util, node
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

def countrevs(ui, repo, amap, revs, progress=False):
    stats = {}
    count = pct = 0
    if not revs:
        revs = range(len(repo))

    for rev in revs:
        ctx2 = repo[rev]
        parents = ctx2.parents()
        if len(parents) > 1:
            ui.note(_('Revision %d is a merge, ignoring...\n') % (rev,))
            continue

        ctx1 = parents[0]
        lines = 0
        ui.pushbuffer()
        patch.diff(repo, ctx1.node(), ctx2.node())
        diff = ui.popbuffer()

        for l in diff.split('\n'):
            if (l.startswith("+") and not l.startswith("+++ ") or
                l.startswith("-") and not l.startswith("--- ")):
                lines += 1

        user = util.email(ctx2.user())
        user = amap.get(user, user) # remap
        stats[user] = stats.get(user, 0) + lines
        ui.debug("rev %d: %d lines by %s\n" % (rev, lines, user))

        if progress:
            count += 1
            newpct = int(100.0 * count / max(len(revs), 1))
            if pct < newpct:
                pct = newpct
                ui.write("\rGenerating stats: %d%%" % pct)
                sys.stdout.flush()

    if progress:
        ui.write("\r")
        sys.stdout.flush()

    return stats

def churn(ui, repo, **opts):
    '''graphs the number of lines changed

    The map file format used to specify aliases is fairly simple:

    <alias email> <actual email>'''

    def pad(s, l):
        return (s + " " * l)[:l]

    amap = {}
    aliases = opts.get('aliases')
    if aliases:
        for l in open(aliases, "r"):
            l = l.strip()
            alias, actual = l.split()
            amap[alias] = actual

    revs = util.sort([int(r) for r in cmdutil.revrange(repo, opts['rev'])])
    stats = countrevs(ui, repo, amap, revs, opts.get('progress'))
    if not stats:
        return

    stats = util.sort([(-l, u, l) for u,l in stats.items()])
    maxchurn = float(max(1, stats[0][2]))
    maxuser = max([len(u) for k, u, l in stats])

    ttywidth = get_tty_width()
    ui.debug(_("assuming %i character terminal\n") % ttywidth)
    width = ttywidth - maxuser - 2 - 6 - 2 - 2

    for k, user, churn in stats:
        print "%s %6d %s" % (pad(user, maxuser), churn,
                             "*" * int(churn * width / maxchurn))

cmdtable = {
    "churn":
    (churn,
     [('r', 'rev', [], _('limit statistics to the specified revisions')),
      ('', 'aliases', '', _('file with email aliases')),
      ('', 'progress', None, _('show progress'))],
    'hg churn [-r revision range] [-a file] [--progress]'),
}
