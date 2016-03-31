# hgweb/webutil.py - utility library for the web interface.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import copy
import difflib
import os
import re

from ..i18n import _
from ..node import hex, nullid, short

from .common import (
    ErrorResponse,
    HTTP_NOT_FOUND,
    paritygen,
)

from .. import (
    context,
    error,
    match,
    patch,
    pathutil,
    templatefilters,
    ui as uimod,
    util,
)

def up(p):
    if p[0] != "/":
        p = "/" + p
    if p[-1] == "/":
        p = p[:-1]
    up = os.path.dirname(p)
    if up == "/":
        return "/"
    return up + "/"

def _navseq(step, firststep=None):
    if firststep:
        yield firststep
        if firststep >= 20 and firststep <= 40:
            firststep = 50
            yield firststep
        assert step > 0
        assert firststep > 0
        while step <= firststep:
            step *= 10
    while True:
        yield 1 * step
        yield 3 * step
        step *= 10

class revnav(object):

    def __init__(self, repo):
        """Navigation generation object

        :repo: repo object we generate nav for
        """
        # used for hex generation
        self._revlog = repo.changelog

    def __nonzero__(self):
        """return True if any revision to navigate over"""
        return self._first() is not None

    def _first(self):
        """return the minimum non-filtered changeset or None"""
        try:
            return iter(self._revlog).next()
        except StopIteration:
            return None

    def hex(self, rev):
        return hex(self._revlog.node(rev))

    def gen(self, pos, pagelen, limit):
        """computes label and revision id for navigation link

        :pos: is the revision relative to which we generate navigation.
        :pagelen: the size of each navigation page
        :limit: how far shall we link

        The return is:
            - a single element tuple
            - containing a dictionary with a `before` and `after` key
            - values are generator functions taking arbitrary number of kwargs
            - yield items are dictionaries with `label` and `node` keys
        """
        if not self:
            # empty repo
            return ({'before': (), 'after': ()},)

        targets = []
        for f in _navseq(1, pagelen):
            if f > limit:
                break
            targets.append(pos + f)
            targets.append(pos - f)
        targets.sort()

        first = self._first()
        navbefore = [("(%i)" % first, self.hex(first))]
        navafter = []
        for rev in targets:
            if rev not in self._revlog:
                continue
            if pos < rev < limit:
                navafter.append(("+%d" % abs(rev - pos), self.hex(rev)))
            if 0 < rev < pos:
                navbefore.append(("-%d" % abs(rev - pos), self.hex(rev)))


        navafter.append(("tip", "tip"))

        data = lambda i: {"label": i[0], "node": i[1]}
        return ({'before': lambda **map: (data(i) for i in navbefore),
                 'after':  lambda **map: (data(i) for i in navafter)},)

class filerevnav(revnav):

    def __init__(self, repo, path):
        """Navigation generation object

        :repo: repo object we generate nav for
        :path: path of the file we generate nav for
        """
        # used for iteration
        self._changelog = repo.unfiltered().changelog
        # used for hex generation
        self._revlog = repo.file(path)

    def hex(self, rev):
        return hex(self._changelog.node(self._revlog.linkrev(rev)))

class _siblings(object):
    def __init__(self, siblings=[], hiderev=None):
        self.siblings = [s for s in siblings if s.node() != nullid]
        if len(self.siblings) == 1 and self.siblings[0].rev() == hiderev:
            self.siblings = []

    def __iter__(self):
        for s in self.siblings:
            d = {
                'node': s.hex(),
                'rev': s.rev(),
                'user': s.user(),
                'date': s.date(),
                'description': s.description(),
                'branch': s.branch(),
            }
            if util.safehasattr(s, 'path'):
                d['file'] = s.path()
            yield d

    def __len__(self):
        return len(self.siblings)

def parents(ctx, hide=None):
    if isinstance(ctx, context.basefilectx):
        introrev = ctx.introrev()
        if ctx.changectx().rev() != introrev:
            return _siblings([ctx.repo()[introrev]], hide)
    return _siblings(ctx.parents(), hide)

def children(ctx, hide=None):
    return _siblings(ctx.children(), hide)

def renamelink(fctx):
    r = fctx.renamed()
    if r:
        return [{'file': r[0], 'node': hex(r[1])}]
    return []

def nodetagsdict(repo, node):
    return [{"name": i} for i in repo.nodetags(node)]

def nodebookmarksdict(repo, node):
    return [{"name": i} for i in repo.nodebookmarks(node)]

def nodebranchdict(repo, ctx):
    branches = []
    branch = ctx.branch()
    # If this is an empty repo, ctx.node() == nullid,
    # ctx.branch() == 'default'.
    try:
        branchnode = repo.branchtip(branch)
    except error.RepoLookupError:
        branchnode = None
    if branchnode == ctx.node():
        branches.append({"name": branch})
    return branches

def nodeinbranch(repo, ctx):
    branches = []
    branch = ctx.branch()
    try:
        branchnode = repo.branchtip(branch)
    except error.RepoLookupError:
        branchnode = None
    if branch != 'default' and branchnode != ctx.node():
        branches.append({"name": branch})
    return branches

def nodebranchnodefault(ctx):
    branches = []
    branch = ctx.branch()
    if branch != 'default':
        branches.append({"name": branch})
    return branches

def showtag(repo, tmpl, t1, node=nullid, **args):
    for t in repo.nodetags(node):
        yield tmpl(t1, tag=t, **args)

def showbookmark(repo, tmpl, t1, node=nullid, **args):
    for t in repo.nodebookmarks(node):
        yield tmpl(t1, bookmark=t, **args)

def branchentries(repo, stripecount, limit=0):
    tips = []
    heads = repo.heads()
    parity = paritygen(stripecount)
    sortkey = lambda item: (not item[1], item[0].rev())

    def entries(**map):
        count = 0
        if not tips:
            for tag, hs, tip, closed in repo.branchmap().iterbranches():
                tips.append((repo[tip], closed))
        for ctx, closed in sorted(tips, key=sortkey, reverse=True):
            if limit > 0 and count >= limit:
                return
            count += 1
            if closed:
                status = 'closed'
            elif ctx.node() not in heads:
                status = 'inactive'
            else:
                status = 'open'
            yield {
                'parity': parity.next(),
                'branch': ctx.branch(),
                'status': status,
                'node': ctx.hex(),
                'date': ctx.date()
            }

    return entries

def cleanpath(repo, path):
    path = path.lstrip('/')
    return pathutil.canonpath(repo.root, '', path)

def changeidctx(repo, changeid):
    try:
        ctx = repo[changeid]
    except error.RepoError:
        man = repo.manifest
        ctx = repo[man.linkrev(man.rev(man.lookup(changeid)))]

    return ctx

def changectx(repo, req):
    changeid = "tip"
    if 'node' in req.form:
        changeid = req.form['node'][0]
        ipos = changeid.find(':')
        if ipos != -1:
            changeid = changeid[(ipos + 1):]
    elif 'manifest' in req.form:
        changeid = req.form['manifest'][0]

    return changeidctx(repo, changeid)

def basechangectx(repo, req):
    if 'node' in req.form:
        changeid = req.form['node'][0]
        ipos = changeid.find(':')
        if ipos != -1:
            changeid = changeid[:ipos]
            return changeidctx(repo, changeid)

    return None

def filectx(repo, req):
    if 'file' not in req.form:
        raise ErrorResponse(HTTP_NOT_FOUND, 'file not given')
    path = cleanpath(repo, req.form['file'][0])
    if 'node' in req.form:
        changeid = req.form['node'][0]
    elif 'filenode' in req.form:
        changeid = req.form['filenode'][0]
    else:
        raise ErrorResponse(HTTP_NOT_FOUND, 'node or filenode not given')
    try:
        fctx = repo[changeid][path]
    except error.RepoError:
        fctx = repo.filectx(path, fileid=changeid)

    return fctx

def commonentry(repo, ctx):
    node = ctx.node()
    return {
        'rev': ctx.rev(),
        'node': hex(node),
        'author': ctx.user(),
        'desc': ctx.description(),
        'date': ctx.date(),
        'extra': ctx.extra(),
        'phase': ctx.phasestr(),
        'branch': nodebranchnodefault(ctx),
        'inbranch': nodeinbranch(repo, ctx),
        'branches': nodebranchdict(repo, ctx),
        'tags': nodetagsdict(repo, node),
        'bookmarks': nodebookmarksdict(repo, node),
        'parent': lambda **x: parents(ctx),
        'child': lambda **x: children(ctx),
    }

def changelistentry(web, ctx, tmpl):
    '''Obtain a dictionary to be used for entries in a changelist.

    This function is called when producing items for the "entries" list passed
    to the "shortlog" and "changelog" templates.
    '''
    repo = web.repo
    rev = ctx.rev()
    n = ctx.node()
    showtags = showtag(repo, tmpl, 'changelogtag', n)
    files = listfilediffs(tmpl, ctx.files(), n, web.maxfiles)

    entry = commonentry(repo, ctx)
    entry.update(
        allparents=lambda **x: parents(ctx),
        parent=lambda **x: parents(ctx, rev - 1),
        child=lambda **x: children(ctx, rev + 1),
        changelogtag=showtags,
        files=files,
    )
    return entry

def symrevorshortnode(req, ctx):
    if 'node' in req.form:
        return templatefilters.revescape(req.form['node'][0])
    else:
        return short(ctx.node())

def changesetentry(web, req, tmpl, ctx):
    '''Obtain a dictionary to be used to render the "changeset" template.'''

    showtags = showtag(web.repo, tmpl, 'changesettag', ctx.node())
    showbookmarks = showbookmark(web.repo, tmpl, 'changesetbookmark',
                                 ctx.node())
    showbranch = nodebranchnodefault(ctx)

    files = []
    parity = paritygen(web.stripecount)
    for blockno, f in enumerate(ctx.files()):
        template = f in ctx and 'filenodelink' or 'filenolink'
        files.append(tmpl(template,
                          node=ctx.hex(), file=f, blockno=blockno + 1,
                          parity=parity.next()))

    basectx = basechangectx(web.repo, req)
    if basectx is None:
        basectx = ctx.p1()

    style = web.config('web', 'style', 'paper')
    if 'style' in req.form:
        style = req.form['style'][0]

    parity = paritygen(web.stripecount)
    diff = diffs(web.repo, tmpl, ctx, basectx, None, parity, style)

    parity = paritygen(web.stripecount)
    diffstatsgen = diffstatgen(ctx, basectx)
    diffstats = diffstat(tmpl, ctx, diffstatsgen, parity)

    return dict(
        diff=diff,
        symrev=symrevorshortnode(req, ctx),
        basenode=basectx.hex(),
        changesettag=showtags,
        changesetbookmark=showbookmarks,
        changesetbranch=showbranch,
        files=files,
        diffsummary=lambda **x: diffsummary(diffstatsgen),
        diffstat=diffstats,
        archives=web.archivelist(ctx.hex()),
        **commonentry(web.repo, ctx))

def listfilediffs(tmpl, files, node, max):
    for f in files[:max]:
        yield tmpl('filedifflink', node=hex(node), file=f)
    if len(files) > max:
        yield tmpl('fileellipses')

def diffs(repo, tmpl, ctx, basectx, files, parity, style):

    def countgen():
        start = 1
        while True:
            yield start
            start += 1

    blockcount = countgen()
    def prettyprintlines(diff, blockno):
        for lineno, l in enumerate(diff.splitlines(True)):
            difflineno = "%d.%d" % (blockno, lineno + 1)
            if l.startswith('+'):
                ltype = "difflineplus"
            elif l.startswith('-'):
                ltype = "difflineminus"
            elif l.startswith('@'):
                ltype = "difflineat"
            else:
                ltype = "diffline"
            yield tmpl(ltype,
                       line=l,
                       lineno=lineno + 1,
                       lineid="l%s" % difflineno,
                       linenumber="% 8s" % difflineno)

    if files:
        m = match.exact(repo.root, repo.getcwd(), files)
    else:
        m = match.always(repo.root, repo.getcwd())

    diffopts = patch.diffopts(repo.ui, untrusted=True)
    if basectx is None:
        parents = ctx.parents()
        if parents:
            node1 = parents[0].node()
        else:
            node1 = nullid
    else:
        node1 = basectx.node()
    node2 = ctx.node()

    block = []
    for chunk in patch.diff(repo, node1, node2, m, opts=diffopts):
        if chunk.startswith('diff') and block:
            blockno = blockcount.next()
            yield tmpl('diffblock', parity=parity.next(), blockno=blockno,
                       lines=prettyprintlines(''.join(block), blockno))
            block = []
        if chunk.startswith('diff') and style != 'raw':
            chunk = ''.join(chunk.splitlines(True)[1:])
        block.append(chunk)
    blockno = blockcount.next()
    yield tmpl('diffblock', parity=parity.next(), blockno=blockno,
               lines=prettyprintlines(''.join(block), blockno))

def compare(tmpl, context, leftlines, rightlines):
    '''Generator function that provides side-by-side comparison data.'''

    def compline(type, leftlineno, leftline, rightlineno, rightline):
        lineid = leftlineno and ("l%s" % leftlineno) or ''
        lineid += rightlineno and ("r%s" % rightlineno) or ''
        return tmpl('comparisonline',
                    type=type,
                    lineid=lineid,
                    leftlineno=leftlineno,
                    leftlinenumber="% 6s" % (leftlineno or ''),
                    leftline=leftline or '',
                    rightlineno=rightlineno,
                    rightlinenumber="% 6s" % (rightlineno or ''),
                    rightline=rightline or '')

    def getblock(opcodes):
        for type, llo, lhi, rlo, rhi in opcodes:
            len1 = lhi - llo
            len2 = rhi - rlo
            count = min(len1, len2)
            for i in xrange(count):
                yield compline(type=type,
                               leftlineno=llo + i + 1,
                               leftline=leftlines[llo + i],
                               rightlineno=rlo + i + 1,
                               rightline=rightlines[rlo + i])
            if len1 > len2:
                for i in xrange(llo + count, lhi):
                    yield compline(type=type,
                                   leftlineno=i + 1,
                                   leftline=leftlines[i],
                                   rightlineno=None,
                                   rightline=None)
            elif len2 > len1:
                for i in xrange(rlo + count, rhi):
                    yield compline(type=type,
                                   leftlineno=None,
                                   leftline=None,
                                   rightlineno=i + 1,
                                   rightline=rightlines[i])

    s = difflib.SequenceMatcher(None, leftlines, rightlines)
    if context < 0:
        yield tmpl('comparisonblock', lines=getblock(s.get_opcodes()))
    else:
        for oc in s.get_grouped_opcodes(n=context):
            yield tmpl('comparisonblock', lines=getblock(oc))

def diffstatgen(ctx, basectx):
    '''Generator function that provides the diffstat data.'''

    stats = patch.diffstatdata(util.iterlines(ctx.diff(basectx)))
    maxname, maxtotal, addtotal, removetotal, binary = patch.diffstatsum(stats)
    while True:
        yield stats, maxname, maxtotal, addtotal, removetotal, binary

def diffsummary(statgen):
    '''Return a short summary of the diff.'''

    stats, maxname, maxtotal, addtotal, removetotal, binary = statgen.next()
    return _(' %d files changed, %d insertions(+), %d deletions(-)\n') % (
             len(stats), addtotal, removetotal)

def diffstat(tmpl, ctx, statgen, parity):
    '''Return a diffstat template for each file in the diff.'''

    stats, maxname, maxtotal, addtotal, removetotal, binary = statgen.next()
    files = ctx.files()

    def pct(i):
        if maxtotal == 0:
            return 0
        return (float(i) / maxtotal) * 100

    fileno = 0
    for filename, adds, removes, isbinary in stats:
        template = filename in files and 'diffstatlink' or 'diffstatnolink'
        total = adds + removes
        fileno += 1
        yield tmpl(template, node=ctx.hex(), file=filename, fileno=fileno,
                   total=total, addpct=pct(adds), removepct=pct(removes),
                   parity=parity.next())

class sessionvars(object):
    def __init__(self, vars, start='?'):
        self.start = start
        self.vars = vars
    def __getitem__(self, key):
        return self.vars[key]
    def __setitem__(self, key, value):
        self.vars[key] = value
    def __copy__(self):
        return sessionvars(copy.copy(self.vars), self.start)
    def __iter__(self):
        separator = self.start
        for key, value in sorted(self.vars.iteritems()):
            yield {'name': key, 'value': str(value), 'separator': separator}
            separator = '&'

class wsgiui(uimod.ui):
    # default termwidth breaks under mod_wsgi
    def termwidth(self):
        return 80

def getwebsubs(repo):
    websubtable = []
    websubdefs = repo.ui.configitems('websub')
    # we must maintain interhg backwards compatibility
    websubdefs += repo.ui.configitems('interhg')
    for key, pattern in websubdefs:
        # grab the delimiter from the character after the "s"
        unesc = pattern[1]
        delim = re.escape(unesc)

        # identify portions of the pattern, taking care to avoid escaped
        # delimiters. the replace format and flags are optional, but
        # delimiters are required.
        match = re.match(
            r'^s%s(.+)(?:(?<=\\\\)|(?<!\\))%s(.*)%s([ilmsux])*$'
            % (delim, delim, delim), pattern)
        if not match:
            repo.ui.warn(_("websub: invalid pattern for %s: %s\n")
                              % (key, pattern))
            continue

        # we need to unescape the delimiter for regexp and format
        delim_re = re.compile(r'(?<!\\)\\%s' % delim)
        regexp = delim_re.sub(unesc, match.group(1))
        format = delim_re.sub(unesc, match.group(2))

        # the pattern allows for 6 regexp flags, so set them if necessary
        flagin = match.group(3)
        flags = 0
        if flagin:
            for flag in flagin.upper():
                flags |= re.__dict__[flag]

        try:
            regexp = re.compile(regexp, flags)
            websubtable.append((regexp, format))
        except re.error:
            repo.ui.warn(_("websub: invalid regexp for %s: %s\n")
                         % (key, regexp))
    return websubtable
