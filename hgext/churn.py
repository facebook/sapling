# churn.py - create a graph of revisions count grouped by template
#
# Copyright 2006 Josef "Jeff" Sipek <jeffpc@josefsipek.net>
# Copyright 2008 Alexander Solovyov <piranha@piranha.org.ua>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

'''command to display statistics about repository history'''

from mercurial.i18n import _
from mercurial import patch, cmdutil, util, templater
import sys, os
import time, datetime

def maketemplater(ui, repo, tmpl):
    tmpl = templater.parsestring(tmpl, quoted=False)
    try:
        t = cmdutil.changeset_templater(ui, repo, False, None, None, False)
    except SyntaxError, inst:
        raise util.Abort(inst.args[0])
    t.use_template(tmpl)
    return t

def changedlines(ui, repo, ctx1, ctx2, fns):
    lines = 0
    fmatch = cmdutil.matchfiles(repo, fns)
    diff = ''.join(patch.diff(repo, ctx1.node(), ctx2.node(), fmatch))
    for l in diff.split('\n'):
        if (l.startswith("+") and not l.startswith("+++ ") or
            l.startswith("-") and not l.startswith("--- ")):
            lines += 1
    return lines

def countrate(ui, repo, amap, *pats, **opts):
    """Calculate stats"""
    if opts.get('dateformat'):
        def getkey(ctx):
            t, tz = ctx.date()
            date = datetime.datetime(*time.gmtime(float(t) - tz)[:6])
            return date.strftime(opts['dateformat'])
    else:
        tmpl = opts.get('template', '{author|email}')
        tmpl = maketemplater(ui, repo, tmpl)
        def getkey(ctx):
            ui.pushbuffer()
            tmpl.show(ctx)
            return ui.popbuffer()

    count = pct = 0
    rate = {}
    df = False
    if opts.get('date'):
        df = util.matchdate(opts['date'])

    get = util.cachefunc(lambda r: repo[r].changeset())
    changeiter, matchfn = cmdutil.walkchangerevs(ui, repo, pats, get, opts)
    for st, rev, fns in changeiter:
        if not st == 'add':
            continue
        if df and not df(get(rev)[2][0]): # doesn't match date format
            continue

        ctx = repo[rev]
        key = getkey(ctx)
        key = amap.get(key, key) # alias remap
        if opts.get('changesets'):
            rate[key] = rate.get(key, 0) + 1
        else:
            parents = ctx.parents()
            if len(parents) > 1:
                ui.note(_('Revision %d is a merge, ignoring...\n') % (rev,))
                continue

            ctx1 = parents[0]
            lines = changedlines(ui, repo, ctx1, ctx, fns)
            rate[key] = rate.get(key, 0) + lines

        if opts.get('progress'):
            count += 1
            newpct = int(100.0 * count / max(len(repo), 1))
            if pct < newpct:
                pct = newpct
                ui.write("\r" + _("generating stats: %d%%") % pct)
                sys.stdout.flush()

    if opts.get('progress'):
        ui.write("\r")
        sys.stdout.flush()

    return rate


def churn(ui, repo, *pats, **opts):
    '''histogram of changes to the repository

    This command will display a histogram representing the number
    of changed lines or revisions, grouped according to the given
    template. The default template will group changes by author.
    The --dateformat option may be used to group the results by
    date instead.

    Statistics are based on the number of changed lines, or
    alternatively the number of matching revisions if the
    --changesets option is specified.

    Examples::

      # display count of changed lines for every committer
      hg churn -t '{author|email}'

      # display daily activity graph
      hg churn -f '%H' -s -c

      # display activity of developers by month
      hg churn -f '%Y-%m' -s -c

      # display count of lines changed in every year
      hg churn -f '%Y' -s

    It is possible to map alternate email addresses to a main address
    by providing a file using the following format::

      <alias email> <actual email>

    Such a file may be specified with the --aliases option, otherwise
    a .hgchurn file will be looked for in the working directory root.
    '''
    def pad(s, l):
        return (s + " " * l)[:l]

    amap = {}
    aliases = opts.get('aliases')
    if not aliases and os.path.exists(repo.wjoin('.hgchurn')):
        aliases = repo.wjoin('.hgchurn')
    if aliases:
        for l in open(aliases, "r"):
            l = l.strip()
            alias, actual = l.split()
            amap[alias] = actual

    rate = countrate(ui, repo, amap, *pats, **opts).items()
    if not rate:
        return

    sortkey = ((not opts.get('sort')) and (lambda x: -x[1]) or None)
    rate.sort(key=sortkey)

    maxcount = float(max([v for k, v in rate]))
    maxname = max([len(k) for k, v in rate])

    ttywidth = util.termwidth()
    ui.debug(_("assuming %i character terminal\n") % ttywidth)
    width = ttywidth - maxname - 2 - 6 - 2 - 2

    for date, count in rate:
        print "%s %6d %s" % (pad(date, maxname), count,
                             "*" * int(count * width / maxcount))


cmdtable = {
    "churn":
        (churn,
         [('r', 'rev', [], _('count rate for the specified revision or range')),
          ('d', 'date', '', _('count rate for revisions matching date spec')),
          ('t', 'template', '{author|email}', _('template to group changesets')),
          ('f', 'dateformat', '',
              _('strftime-compatible format for grouping by date')),
          ('c', 'changesets', False, _('count rate by number of changesets')),
          ('s', 'sort', False, _('sort by key (default: sort by count)')),
          ('', 'aliases', '', _('file with email aliases')),
          ('', 'progress', None, _('show progress'))],
         _("hg churn [-d DATE] [-r REV] [--aliases FILE] [--progress] [FILE]")),
}
