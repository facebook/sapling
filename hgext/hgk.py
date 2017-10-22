# Minimal support for git commands on an hg repository
#
# Copyright 2005, 2006 Chris Mason <mason@suse.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''browse the repository in a graphical way

The hgk extension allows browsing the history of a repository in a
graphical way. It requires Tcl/Tk version 8.4 or later. (Tcl/Tk is not
distributed with Mercurial.)

hgk consists of two parts: a Tcl script that does the displaying and
querying of information, and an extension to Mercurial named hgk.py,
which provides hooks for hgk to get information. hgk can be found in
the contrib directory, and the extension is shipped in the hgext
repository, and needs to be enabled.

The :hg:`view` command will launch the hgk Tcl script. For this command
to work, hgk must be in your search path. Alternately, you can specify
the path to hgk in your configuration file::

  [hgk]
  path = /location/of/hgk

hgk can make use of the extdiff extension to visualize revisions.
Assuming you had already configured extdiff vdiff command, just add::

  [hgk]
  vdiff=vdiff

Revisions context menu will now display additional entries to fire
vdiff on hovered and selected revisions.
'''

from __future__ import absolute_import

import os

from mercurial.i18n import _
from mercurial.node import (
    nullid,
    nullrev,
    short,
)
from mercurial import (
    commands,
    obsolete,
    patch,
    pycompat,
    registrar,
    scmutil,
    util,
)

cmdtable = {}
command = registrar.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

configtable = {}
configitem = registrar.configitem(configtable)

configitem('hgk', 'path',
    default='hgk',
)

@command('debug-diff-tree',
    [('p', 'patch', None, _('generate patch')),
    ('r', 'recursive', None, _('recursive')),
    ('P', 'pretty', None, _('pretty')),
    ('s', 'stdin', None, _('stdin')),
    ('C', 'copy', None, _('detect copies')),
    ('S', 'search', "", _('search'))],
    ('[OPTION]... NODE1 NODE2 [FILE]...'),
    inferrepo=True)
def difftree(ui, repo, node1=None, node2=None, *files, **opts):
    """diff trees from two commits"""

    def __difftree(repo, node1, node2, files=None):
        assert node2 is not None
        if files is None:
            files = []
        mmap = repo[node1].manifest()
        mmap2 = repo[node2].manifest()
        m = scmutil.match(repo[node1], files)
        modified, added, removed  = repo.status(node1, node2, m)[:3]
        empty = short(nullid)

        for f in modified:
            # TODO get file permissions
            ui.write((":100664 100664 %s %s M\t%s\t%s\n") %
                     (short(mmap[f]), short(mmap2[f]), f, f))
        for f in added:
            ui.write((":000000 100664 %s %s N\t%s\t%s\n") %
                     (empty, short(mmap2[f]), f, f))
        for f in removed:
            ui.write((":100664 000000 %s %s D\t%s\t%s\n") %
                     (short(mmap[f]), empty, f, f))
    ##

    while True:
        if opts[r'stdin']:
            try:
                line = util.bytesinput(ui.fin, ui.fout).split(' ')
                node1 = line[0]
                if len(line) > 1:
                    node2 = line[1]
                else:
                    node2 = None
            except EOFError:
                break
        node1 = repo.lookup(node1)
        if node2:
            node2 = repo.lookup(node2)
        else:
            node2 = node1
            node1 = repo.changelog.parents(node1)[0]
        if opts[r'patch']:
            if opts[r'pretty']:
                catcommit(ui, repo, node2, "")
            m = scmutil.match(repo[node1], files)
            diffopts = patch.difffeatureopts(ui)
            diffopts.git = True
            chunks = patch.diff(repo, node1, node2, match=m,
                                opts=diffopts)
            for chunk in chunks:
                ui.write(chunk)
        else:
            __difftree(repo, node1, node2, files=files)
        if not opts[r'stdin']:
            break

def catcommit(ui, repo, n, prefix, ctx=None):
    nlprefix = '\n' + prefix
    if ctx is None:
        ctx = repo[n]
    # use ctx.node() instead ??
    ui.write(("tree %s\n" % short(ctx.changeset()[0])))
    for p in ctx.parents():
        ui.write(("parent %s\n" % p))

    date = ctx.date()
    description = ctx.description().replace("\0", "")
    ui.write(("author %s %s %s\n" % (ctx.user(), int(date[0]), date[1])))

    if 'committer' in ctx.extra():
        ui.write(("committer %s\n" % ctx.extra()['committer']))

    ui.write(("revision %d\n" % ctx.rev()))
    ui.write(("branch %s\n" % ctx.branch()))
    if obsolete.isenabled(repo, obsolete.createmarkersopt):
        if ctx.obsolete():
            ui.write(("obsolete\n"))
    ui.write(("phase %s\n\n" % ctx.phasestr()))

    if prefix != "":
        ui.write("%s%s\n" % (prefix,
                             description.replace('\n', nlprefix).strip()))
    else:
        ui.write(description + "\n")
    if prefix:
        ui.write('\0')

@command('debug-merge-base', [], _('REV REV'))
def base(ui, repo, node1, node2):
    """output common ancestor information"""
    node1 = repo.lookup(node1)
    node2 = repo.lookup(node2)
    n = repo.changelog.ancestor(node1, node2)
    ui.write(short(n) + "\n")

@command('debug-cat-file',
    [('s', 'stdin', None, _('stdin'))],
    _('[OPTION]... TYPE FILE'),
    inferrepo=True)
def catfile(ui, repo, type=None, r=None, **opts):
    """cat a specific revision"""
    # in stdin mode, every line except the commit is prefixed with two
    # spaces.  This way the our caller can find the commit without magic
    # strings
    #
    prefix = ""
    if opts[r'stdin']:
        try:
            (type, r) = util.bytesinput(ui.fin, ui.fout).split(' ')
            prefix = "    "
        except EOFError:
            return

    else:
        if not type or not r:
            ui.warn(_("cat-file: type or revision not supplied\n"))
            commands.help_(ui, 'cat-file')

    while r:
        if type != "commit":
            ui.warn(_("aborting hg cat-file only understands commits\n"))
            return 1
        n = repo.lookup(r)
        catcommit(ui, repo, n, prefix)
        if opts[r'stdin']:
            try:
                (type, r) = util.bytesinput(ui.fin, ui.fout).split(' ')
            except EOFError:
                break
        else:
            break

# git rev-tree is a confusing thing.  You can supply a number of
# commit sha1s on the command line, and it walks the commit history
# telling you which commits are reachable from the supplied ones via
# a bitmask based on arg position.
# you can specify a commit to stop at by starting the sha1 with ^
def revtree(ui, args, repo, full="tree", maxnr=0, parents=False):
    def chlogwalk():
        count = len(repo)
        i = count
        l = [0] * 100
        chunk = 100
        while True:
            if chunk > i:
                chunk = i
                i = 0
            else:
                i -= chunk

            for x in xrange(chunk):
                if i + x >= count:
                    l[chunk - x:] = [0] * (chunk - x)
                    break
                if full is not None:
                    if (i + x) in repo:
                        l[x] = repo[i + x]
                        l[x].changeset() # force reading
                else:
                    if (i + x) in repo:
                        l[x] = 1
            for x in xrange(chunk - 1, -1, -1):
                if l[x] != 0:
                    yield (i + x, full is not None and l[x] or None)
            if i == 0:
                break

    # calculate and return the reachability bitmask for sha
    def is_reachable(ar, reachable, sha):
        if len(ar) == 0:
            return 1
        mask = 0
        for i in xrange(len(ar)):
            if sha in reachable[i]:
                mask |= 1 << i

        return mask

    reachable = []
    stop_sha1 = []
    want_sha1 = []
    count = 0

    # figure out which commits they are asking for and which ones they
    # want us to stop on
    for i, arg in enumerate(args):
        if arg.startswith('^'):
            s = repo.lookup(arg[1:])
            stop_sha1.append(s)
            want_sha1.append(s)
        elif arg != 'HEAD':
            want_sha1.append(repo.lookup(arg))

    # calculate the graph for the supplied commits
    for i, n in enumerate(want_sha1):
        reachable.append(set())
        visit = [n]
        reachable[i].add(n)
        while visit:
            n = visit.pop(0)
            if n in stop_sha1:
                continue
            for p in repo.changelog.parents(n):
                if p not in reachable[i]:
                    reachable[i].add(p)
                    visit.append(p)
                if p in stop_sha1:
                    continue

    # walk the repository looking for commits that are in our
    # reachability graph
    for i, ctx in chlogwalk():
        if i not in repo:
            continue
        n = repo.changelog.node(i)
        mask = is_reachable(want_sha1, reachable, n)
        if mask:
            parentstr = ""
            if parents:
                pp = repo.changelog.parents(n)
                if pp[0] != nullid:
                    parentstr += " " + short(pp[0])
                if pp[1] != nullid:
                    parentstr += " " + short(pp[1])
            if not full:
                ui.write("%s%s\n" % (short(n), parentstr))
            elif full == "commit":
                ui.write("%s%s\n" % (short(n), parentstr))
                catcommit(ui, repo, n, '    ', ctx)
            else:
                (p1, p2) = repo.changelog.parents(n)
                (h, h1, h2) = map(short, (n, p1, p2))
                (i1, i2) = map(repo.changelog.rev, (p1, p2))

                date = ctx.date()[0]
                ui.write("%s %s:%s" % (date, h, mask))
                mask = is_reachable(want_sha1, reachable, p1)
                if i1 != nullrev and mask > 0:
                    ui.write("%s:%s " % (h1, mask)),
                mask = is_reachable(want_sha1, reachable, p2)
                if i2 != nullrev and mask > 0:
                    ui.write("%s:%s " % (h2, mask))
                ui.write("\n")
            if maxnr and count >= maxnr:
                break
            count += 1

# git rev-list tries to order things by date, and has the ability to stop
# at a given commit without walking the whole repo.  TODO add the stop
# parameter
@command('debug-rev-list',
    [('H', 'header', None, _('header')),
    ('t', 'topo-order', None, _('topo-order')),
    ('p', 'parents', None, _('parents')),
    ('n', 'max-count', 0, _('max-count'))],
    ('[OPTION]... REV...'))
def revlist(ui, repo, *revs, **opts):
    """print revisions"""
    if opts['header']:
        full = "commit"
    else:
        full = None
    copy = [x for x in revs]
    revtree(ui, copy, repo, full, opts[r'max_count'], opts[r'parents'])

@command('view',
    [('l', 'limit', '',
     _('limit number of changes displayed'), _('NUM'))],
    _('[-l LIMIT] [REVRANGE]'))
def view(ui, repo, *etc, **opts):
    "start interactive history viewer"
    opts = pycompat.byteskwargs(opts)
    os.chdir(repo.root)
    optstr = ' '.join(['--%s %s' % (k, v) for k, v in opts.iteritems() if v])
    if repo.filtername is None:
        optstr += '--hidden'

    cmd = ui.config("hgk", "path") + " %s %s" % (optstr, " ".join(etc))
    ui.debug("running %s\n" % cmd)
    ui.system(cmd, blockedtag='hgk_view')
