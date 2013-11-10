#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os, mimetypes, re, cgi, copy
import webutil
from mercurial import error, encoding, archival, templater, templatefilters
from mercurial.node import short, hex, nullid
from mercurial import util
from common import paritygen, staticfile, get_contact, ErrorResponse
from common import HTTP_OK, HTTP_FORBIDDEN, HTTP_NOT_FOUND
from mercurial import graphmod, patch
from mercurial import help as helpmod
from mercurial import scmutil
from mercurial.i18n import _
from mercurial.error import ParseError, RepoLookupError, Abort
from mercurial import revset

# __all__ is populated with the allowed commands. Be sure to add to it if
# you're adding a new command, or the new command won't work.

__all__ = [
   'log', 'rawfile', 'file', 'changelog', 'shortlog', 'changeset', 'rev',
   'manifest', 'tags', 'bookmarks', 'branches', 'summary', 'filediff', 'diff',
   'comparison', 'annotate', 'filelog', 'archive', 'static', 'graph', 'help',
]

def log(web, req, tmpl):
    if 'file' in req.form and req.form['file'][0]:
        return filelog(web, req, tmpl)
    else:
        return changelog(web, req, tmpl)

def rawfile(web, req, tmpl):
    guessmime = web.configbool('web', 'guessmime', False)

    path = webutil.cleanpath(web.repo, req.form.get('file', [''])[0])
    if not path:
        content = manifest(web, req, tmpl)
        req.respond(HTTP_OK, web.ctype)
        return content

    try:
        fctx = webutil.filectx(web.repo, req)
    except error.LookupError, inst:
        try:
            content = manifest(web, req, tmpl)
            req.respond(HTTP_OK, web.ctype)
            return content
        except ErrorResponse:
            raise inst

    path = fctx.path()
    text = fctx.data()
    mt = 'application/binary'
    if guessmime:
        mt = mimetypes.guess_type(path)[0]
        if mt is None:
            mt = util.binary(text) and 'application/binary' or 'text/plain'
    if mt.startswith('text/'):
        mt += '; charset="%s"' % encoding.encoding

    req.respond(HTTP_OK, mt, path, body=text)
    return []

def _filerevision(web, tmpl, fctx):
    f = fctx.path()
    text = fctx.data()
    parity = paritygen(web.stripecount)

    if util.binary(text):
        mt = mimetypes.guess_type(f)[0] or 'application/octet-stream'
        text = '(binary:%s)' % mt

    def lines():
        for lineno, t in enumerate(text.splitlines(True)):
            yield {"line": t,
                   "lineid": "l%d" % (lineno + 1),
                   "linenumber": "% 6d" % (lineno + 1),
                   "parity": parity.next()}

    return tmpl("filerevision",
                file=f,
                path=webutil.up(f),
                text=lines(),
                rev=fctx.rev(),
                node=fctx.hex(),
                author=fctx.user(),
                date=fctx.date(),
                desc=fctx.description(),
                extra=fctx.extra(),
                branch=webutil.nodebranchnodefault(fctx),
                parent=webutil.parents(fctx),
                child=webutil.children(fctx),
                rename=webutil.renamelink(fctx),
                permissions=fctx.manifest().flags(f))

def file(web, req, tmpl):
    path = webutil.cleanpath(web.repo, req.form.get('file', [''])[0])
    if not path:
        return manifest(web, req, tmpl)
    try:
        return _filerevision(web, tmpl, webutil.filectx(web.repo, req))
    except error.LookupError, inst:
        try:
            return manifest(web, req, tmpl)
        except ErrorResponse:
            raise inst

def _search(web, req, tmpl):
    MODE_REVISION = 'rev'
    MODE_KEYWORD = 'keyword'
    MODE_REVSET = 'revset'

    def revsearch(ctx):
        yield ctx

    def keywordsearch(query):
        lower = encoding.lower
        qw = lower(query).split()

        def revgen():
            cl = web.repo.changelog
            for i in xrange(len(web.repo) - 1, 0, -100):
                l = []
                for j in cl.revs(max(0, i - 99), i):
                    ctx = web.repo[j]
                    l.append(ctx)
                l.reverse()
                for e in l:
                    yield e

        for ctx in revgen():
            miss = 0
            for q in qw:
                if not (q in lower(ctx.user()) or
                        q in lower(ctx.description()) or
                        q in lower(" ".join(ctx.files()))):
                    miss = 1
                    break
            if miss:
                continue

            yield ctx

    def revsetsearch(revs):
        for r in revs:
            yield web.repo[r]

    searchfuncs = {
        MODE_REVISION: (revsearch, 'exact revision search'),
        MODE_KEYWORD: (keywordsearch, 'literal keyword search'),
        MODE_REVSET: (revsetsearch, 'revset expression search'),
    }

    def getsearchmode(query):
        try:
            ctx = web.repo[query]
        except (error.RepoError, error.LookupError):
            # query is not an exact revision pointer, need to
            # decide if it's a revset expression or keywords
            pass
        else:
            return MODE_REVISION, ctx

        revdef = 'reverse(%s)' % query
        try:
            tree, pos = revset.parse(revdef)
        except ParseError:
            # can't parse to a revset tree
            return MODE_KEYWORD, query

        if revset.depth(tree) <= 2:
            # no revset syntax used
            return MODE_KEYWORD, query

        if util.any((token, (value or '')[:3]) == ('string', 're:')
                    for token, value, pos in revset.tokenize(revdef)):
            return MODE_KEYWORD, query

        funcsused = revset.funcsused(tree)
        if not funcsused.issubset(revset.safesymbols):
            return MODE_KEYWORD, query

        mfunc = revset.match(web.repo.ui, revdef)
        try:
            revs = mfunc(web.repo, list(web.repo))
            return MODE_REVSET, revs
            # ParseError: wrongly placed tokens, wrongs arguments, etc
            # RepoLookupError: no such revision, e.g. in 'revision:'
            # Abort: bookmark/tag not exists
            # LookupError: ambiguous identifier, e.g. in '(bc)' on a large repo
        except (ParseError, RepoLookupError, Abort, LookupError):
            return MODE_KEYWORD, query

    def changelist(**map):
        count = 0

        for ctx in searchfunc[0](funcarg):
            count += 1
            n = ctx.node()
            showtags = webutil.showtag(web.repo, tmpl, 'changelogtag', n)
            files = webutil.listfilediffs(tmpl, ctx.files(), n, web.maxfiles)

            yield tmpl('searchentry',
                       parity=parity.next(),
                       author=ctx.user(),
                       parent=webutil.parents(ctx),
                       child=webutil.children(ctx),
                       changelogtag=showtags,
                       desc=ctx.description(),
                       extra=ctx.extra(),
                       date=ctx.date(),
                       files=files,
                       rev=ctx.rev(),
                       node=hex(n),
                       tags=webutil.nodetagsdict(web.repo, n),
                       bookmarks=webutil.nodebookmarksdict(web.repo, n),
                       inbranch=webutil.nodeinbranch(web.repo, ctx),
                       branches=webutil.nodebranchdict(web.repo, ctx))

            if count >= revcount:
                break

    query = req.form['rev'][0]
    revcount = web.maxchanges
    if 'revcount' in req.form:
        revcount = int(req.form.get('revcount', [revcount])[0])
        revcount = max(revcount, 1)
        tmpl.defaults['sessionvars']['revcount'] = revcount

    lessvars = copy.copy(tmpl.defaults['sessionvars'])
    lessvars['revcount'] = max(revcount / 2, 1)
    lessvars['rev'] = query
    morevars = copy.copy(tmpl.defaults['sessionvars'])
    morevars['revcount'] = revcount * 2
    morevars['rev'] = query

    mode, funcarg = getsearchmode(query)

    if 'forcekw' in req.form:
        showforcekw = ''
        showunforcekw = searchfuncs[mode][1]
        mode = MODE_KEYWORD
        funcarg = query
    else:
        if mode != MODE_KEYWORD:
            showforcekw = searchfuncs[MODE_KEYWORD][1]
        else:
            showforcekw = ''
        showunforcekw = ''

    searchfunc = searchfuncs[mode]

    tip = web.repo['tip']
    parity = paritygen(web.stripecount)

    return tmpl('search', query=query, node=tip.hex(),
                entries=changelist, archives=web.archivelist("tip"),
                morevars=morevars, lessvars=lessvars,
                modedesc=searchfunc[1],
                showforcekw=showforcekw, showunforcekw=showunforcekw)

def changelog(web, req, tmpl, shortlog=False):

    query = ''
    if 'node' in req.form:
        ctx = webutil.changectx(web.repo, req)
    elif 'rev' in req.form:
        return _search(web, req, tmpl)
    else:
        ctx = web.repo['tip']

    def changelist():
        revs = []
        if pos != -1:
            revs = web.repo.changelog.revs(pos, 0)
        curcount = 0
        for i in revs:
            ctx = web.repo[i]
            n = ctx.node()
            showtags = webutil.showtag(web.repo, tmpl, 'changelogtag', n)
            files = webutil.listfilediffs(tmpl, ctx.files(), n, web.maxfiles)

            curcount += 1
            if curcount > revcount + 1:
                break
            yield {"parity": parity.next(),
                   "author": ctx.user(),
                   "parent": webutil.parents(ctx, i - 1),
                   "child": webutil.children(ctx, i + 1),
                   "changelogtag": showtags,
                   "desc": ctx.description(),
                   "extra": ctx.extra(),
                   "date": ctx.date(),
                   "files": files,
                   "rev": i,
                   "node": hex(n),
                   "tags": webutil.nodetagsdict(web.repo, n),
                   "bookmarks": webutil.nodebookmarksdict(web.repo, n),
                   "inbranch": webutil.nodeinbranch(web.repo, ctx),
                   "branches": webutil.nodebranchdict(web.repo, ctx)
            }

    revcount = shortlog and web.maxshortchanges or web.maxchanges
    if 'revcount' in req.form:
        revcount = int(req.form.get('revcount', [revcount])[0])
        revcount = max(revcount, 1)
        tmpl.defaults['sessionvars']['revcount'] = revcount

    lessvars = copy.copy(tmpl.defaults['sessionvars'])
    lessvars['revcount'] = max(revcount / 2, 1)
    morevars = copy.copy(tmpl.defaults['sessionvars'])
    morevars['revcount'] = revcount * 2

    count = len(web.repo)
    pos = ctx.rev()
    parity = paritygen(web.stripecount)

    changenav = webutil.revnav(web.repo).gen(pos, revcount, count)

    entries = list(changelist())
    latestentry = entries[:1]
    if len(entries) > revcount:
        nextentry = entries[-1:]
        entries = entries[:-1]
    else:
        nextentry = []

    return tmpl(shortlog and 'shortlog' or 'changelog', changenav=changenav,
                node=ctx.hex(), rev=pos, changesets=count,
                entries=entries,
                latestentry=latestentry, nextentry=nextentry,
                archives=web.archivelist("tip"), revcount=revcount,
                morevars=morevars, lessvars=lessvars, query=query)

def shortlog(web, req, tmpl):
    return changelog(web, req, tmpl, shortlog=True)

def changeset(web, req, tmpl):
    ctx = webutil.changectx(web.repo, req)
    basectx = webutil.basechangectx(web.repo, req)
    if basectx is None:
        basectx = ctx.p1()
    showtags = webutil.showtag(web.repo, tmpl, 'changesettag', ctx.node())
    showbookmarks = webutil.showbookmark(web.repo, tmpl, 'changesetbookmark',
                                         ctx.node())
    showbranch = webutil.nodebranchnodefault(ctx)

    files = []
    parity = paritygen(web.stripecount)
    for blockno, f in enumerate(ctx.files()):
        template = f in ctx and 'filenodelink' or 'filenolink'
        files.append(tmpl(template,
                          node=ctx.hex(), file=f, blockno=blockno + 1,
                          parity=parity.next()))

    style = web.config('web', 'style', 'paper')
    if 'style' in req.form:
        style = req.form['style'][0]

    parity = paritygen(web.stripecount)
    diffs = webutil.diffs(web.repo, tmpl, ctx, basectx, None, parity, style)

    parity = paritygen(web.stripecount)
    diffstatgen = webutil.diffstatgen(ctx, basectx)
    diffstat = webutil.diffstat(tmpl, ctx, diffstatgen, parity)

    return tmpl('changeset',
                diff=diffs,
                rev=ctx.rev(),
                node=ctx.hex(),
                parent=webutil.parents(ctx),
                child=webutil.children(ctx),
                basenode=basectx.hex(),
                changesettag=showtags,
                changesetbookmark=showbookmarks,
                changesetbranch=showbranch,
                author=ctx.user(),
                desc=ctx.description(),
                extra=ctx.extra(),
                date=ctx.date(),
                files=files,
                diffsummary=lambda **x: webutil.diffsummary(diffstatgen),
                diffstat=diffstat,
                archives=web.archivelist(ctx.hex()),
                tags=webutil.nodetagsdict(web.repo, ctx.node()),
                bookmarks=webutil.nodebookmarksdict(web.repo, ctx.node()),
                branch=webutil.nodebranchnodefault(ctx),
                inbranch=webutil.nodeinbranch(web.repo, ctx),
                branches=webutil.nodebranchdict(web.repo, ctx))

rev = changeset

def decodepath(path):
    """Hook for mapping a path in the repository to a path in the
    working copy.

    Extensions (e.g., largefiles) can override this to remap files in
    the virtual file system presented by the manifest command below."""
    return path

def manifest(web, req, tmpl):
    ctx = webutil.changectx(web.repo, req)
    path = webutil.cleanpath(web.repo, req.form.get('file', [''])[0])
    mf = ctx.manifest()
    node = ctx.node()

    files = {}
    dirs = {}
    parity = paritygen(web.stripecount)

    if path and path[-1] != "/":
        path += "/"
    l = len(path)
    abspath = "/" + path

    for full, n in mf.iteritems():
        # the virtual path (working copy path) used for the full
        # (repository) path
        f = decodepath(full)

        if f[:l] != path:
            continue
        remain = f[l:]
        elements = remain.split('/')
        if len(elements) == 1:
            files[remain] = full
        else:
            h = dirs # need to retain ref to dirs (root)
            for elem in elements[0:-1]:
                if elem not in h:
                    h[elem] = {}
                h = h[elem]
                if len(h) > 1:
                    break
            h[None] = None # denotes files present

    if mf and not files and not dirs:
        raise ErrorResponse(HTTP_NOT_FOUND, 'path not found: ' + path)

    def filelist(**map):
        for f in sorted(files):
            full = files[f]

            fctx = ctx.filectx(full)
            yield {"file": full,
                   "parity": parity.next(),
                   "basename": f,
                   "date": fctx.date(),
                   "size": fctx.size(),
                   "permissions": mf.flags(full)}

    def dirlist(**map):
        for d in sorted(dirs):

            emptydirs = []
            h = dirs[d]
            while isinstance(h, dict) and len(h) == 1:
                k, v = h.items()[0]
                if v:
                    emptydirs.append(k)
                h = v

            path = "%s%s" % (abspath, d)
            yield {"parity": parity.next(),
                   "path": path,
                   "emptydirs": "/".join(emptydirs),
                   "basename": d}

    return tmpl("manifest",
                rev=ctx.rev(),
                node=hex(node),
                path=abspath,
                up=webutil.up(abspath),
                upparity=parity.next(),
                fentries=filelist,
                dentries=dirlist,
                archives=web.archivelist(hex(node)),
                tags=webutil.nodetagsdict(web.repo, node),
                bookmarks=webutil.nodebookmarksdict(web.repo, node),
                inbranch=webutil.nodeinbranch(web.repo, ctx),
                branches=webutil.nodebranchdict(web.repo, ctx))

def tags(web, req, tmpl):
    i = list(reversed(web.repo.tagslist()))
    parity = paritygen(web.stripecount)

    def entries(notip, latestonly, **map):
        t = i
        if notip:
            t = [(k, n) for k, n in i if k != "tip"]
        if latestonly:
            t = t[:1]
        for k, n in t:
            yield {"parity": parity.next(),
                   "tag": k,
                   "date": web.repo[n].date(),
                   "node": hex(n)}

    return tmpl("tags",
                node=hex(web.repo.changelog.tip()),
                entries=lambda **x: entries(False, False, **x),
                entriesnotip=lambda **x: entries(True, False, **x),
                latestentry=lambda **x: entries(True, True, **x))

def bookmarks(web, req, tmpl):
    i = [b for b in web.repo._bookmarks.items() if b[1] in web.repo]
    parity = paritygen(web.stripecount)

    def entries(latestonly, **map):
        if latestonly:
            t = [min(i)]
        else:
            t = sorted(i)
        for k, n in t:
            yield {"parity": parity.next(),
                   "bookmark": k,
                   "date": web.repo[n].date(),
                   "node": hex(n)}

    return tmpl("bookmarks",
                node=hex(web.repo.changelog.tip()),
                entries=lambda **x: entries(latestonly=False, **x),
                latestentry=lambda **x: entries(latestonly=True, **x))

def branches(web, req, tmpl):
    tips = []
    heads = web.repo.heads()
    parity = paritygen(web.stripecount)
    sortkey = lambda ctx: (not ctx.closesbranch(), ctx.rev())

    def entries(limit, **map):
        count = 0
        if not tips:
            for t, n in web.repo.branchtags().iteritems():
                tips.append(web.repo[n])
        for ctx in sorted(tips, key=sortkey, reverse=True):
            if limit > 0 and count >= limit:
                return
            count += 1
            if not web.repo.branchheads(ctx.branch()):
                status = 'closed'
            elif ctx.node() not in heads:
                status = 'inactive'
            else:
                status = 'open'
            yield {'parity': parity.next(),
                   'branch': ctx.branch(),
                   'status': status,
                   'node': ctx.hex(),
                   'date': ctx.date()}

    return tmpl('branches', node=hex(web.repo.changelog.tip()),
                entries=lambda **x: entries(0, **x),
                latestentry=lambda **x: entries(1, **x))

def summary(web, req, tmpl):
    i = reversed(web.repo.tagslist())

    def tagentries(**map):
        parity = paritygen(web.stripecount)
        count = 0
        for k, n in i:
            if k == "tip": # skip tip
                continue

            count += 1
            if count > 10: # limit to 10 tags
                break

            yield tmpl("tagentry",
                       parity=parity.next(),
                       tag=k,
                       node=hex(n),
                       date=web.repo[n].date())

    def bookmarks(**map):
        parity = paritygen(web.stripecount)
        marks = [b for b in web.repo._bookmarks.items() if b[1] in web.repo]
        for k, n in sorted(marks)[:10]:  # limit to 10 bookmarks
            yield {'parity': parity.next(),
                   'bookmark': k,
                   'date': web.repo[n].date(),
                   'node': hex(n)}

    def branches(**map):
        parity = paritygen(web.stripecount)

        b = web.repo.branchtags()
        l = [(-web.repo.changelog.rev(n), n, t) for t, n in b.iteritems()]
        for r, n, t in sorted(l):
            yield {'parity': parity.next(),
                   'branch': t,
                   'node': hex(n),
                   'date': web.repo[n].date()}

    def changelist(**map):
        parity = paritygen(web.stripecount, offset=start - end)
        l = [] # build a list in forward order for efficiency
        revs = []
        if start < end:
            revs = web.repo.changelog.revs(start, end - 1)
        for i in revs:
            ctx = web.repo[i]
            n = ctx.node()
            hn = hex(n)

            l.append(tmpl(
               'shortlogentry',
                parity=parity.next(),
                author=ctx.user(),
                desc=ctx.description(),
                extra=ctx.extra(),
                date=ctx.date(),
                rev=i,
                node=hn,
                tags=webutil.nodetagsdict(web.repo, n),
                bookmarks=webutil.nodebookmarksdict(web.repo, n),
                inbranch=webutil.nodeinbranch(web.repo, ctx),
                branches=webutil.nodebranchdict(web.repo, ctx)))

        l.reverse()
        yield l

    tip = web.repo['tip']
    count = len(web.repo)
    start = max(0, count - web.maxchanges)
    end = min(count, start + web.maxchanges)

    return tmpl("summary",
                desc=web.config("web", "description", "unknown"),
                owner=get_contact(web.config) or "unknown",
                lastchange=tip.date(),
                tags=tagentries,
                bookmarks=bookmarks,
                branches=branches,
                shortlog=changelist,
                node=tip.hex(),
                archives=web.archivelist("tip"))

def filediff(web, req, tmpl):
    fctx, ctx = None, None
    try:
        fctx = webutil.filectx(web.repo, req)
    except LookupError:
        ctx = webutil.changectx(web.repo, req)
        path = webutil.cleanpath(web.repo, req.form['file'][0])
        if path not in ctx.files():
            raise

    if fctx is not None:
        n = fctx.node()
        path = fctx.path()
        ctx = fctx.changectx()
    else:
        n = ctx.node()
        # path already defined in except clause

    parity = paritygen(web.stripecount)
    style = web.config('web', 'style', 'paper')
    if 'style' in req.form:
        style = req.form['style'][0]

    diffs = webutil.diffs(web.repo, tmpl, ctx, None, [path], parity, style)
    rename = fctx and webutil.renamelink(fctx) or []
    ctx = fctx and fctx or ctx
    return tmpl("filediff",
                file=path,
                node=hex(n),
                rev=ctx.rev(),
                date=ctx.date(),
                desc=ctx.description(),
                extra=ctx.extra(),
                author=ctx.user(),
                rename=rename,
                branch=webutil.nodebranchnodefault(ctx),
                parent=webutil.parents(ctx),
                child=webutil.children(ctx),
                diff=diffs)

diff = filediff

def comparison(web, req, tmpl):
    ctx = webutil.changectx(web.repo, req)
    if 'file' not in req.form:
        raise ErrorResponse(HTTP_NOT_FOUND, 'file not given')
    path = webutil.cleanpath(web.repo, req.form['file'][0])
    rename = path in ctx and webutil.renamelink(ctx[path]) or []

    parsecontext = lambda v: v == 'full' and -1 or int(v)
    if 'context' in req.form:
        context = parsecontext(req.form['context'][0])
    else:
        context = parsecontext(web.config('web', 'comparisoncontext', '5'))

    def filelines(f):
        if util.binary(f.data()):
            mt = mimetypes.guess_type(f.path())[0]
            if not mt:
                mt = 'application/octet-stream'
            return [_('(binary file %s, hash: %s)') % (mt, hex(f.filenode()))]
        return f.data().splitlines()

    if path in ctx:
        fctx = ctx[path]
        rightrev = fctx.filerev()
        rightnode = fctx.filenode()
        rightlines = filelines(fctx)
        parents = fctx.parents()
        if not parents:
            leftrev = -1
            leftnode = nullid
            leftlines = ()
        else:
            pfctx = parents[0]
            leftrev = pfctx.filerev()
            leftnode = pfctx.filenode()
            leftlines = filelines(pfctx)
    else:
        rightrev = -1
        rightnode = nullid
        rightlines = ()
        fctx = ctx.parents()[0][path]
        leftrev = fctx.filerev()
        leftnode = fctx.filenode()
        leftlines = filelines(fctx)

    comparison = webutil.compare(tmpl, context, leftlines, rightlines)
    return tmpl('filecomparison',
                file=path,
                node=hex(ctx.node()),
                rev=ctx.rev(),
                date=ctx.date(),
                desc=ctx.description(),
                extra=ctx.extra(),
                author=ctx.user(),
                rename=rename,
                branch=webutil.nodebranchnodefault(ctx),
                parent=webutil.parents(fctx),
                child=webutil.children(fctx),
                leftrev=leftrev,
                leftnode=hex(leftnode),
                rightrev=rightrev,
                rightnode=hex(rightnode),
                comparison=comparison)

def annotate(web, req, tmpl):
    fctx = webutil.filectx(web.repo, req)
    f = fctx.path()
    parity = paritygen(web.stripecount)
    diffopts = patch.diffopts(web.repo.ui, untrusted=True, section='annotate')

    def annotate(**map):
        last = None
        if util.binary(fctx.data()):
            mt = (mimetypes.guess_type(fctx.path())[0]
                  or 'application/octet-stream')
            lines = enumerate([((fctx.filectx(fctx.filerev()), 1),
                                '(binary:%s)' % mt)])
        else:
            lines = enumerate(fctx.annotate(follow=True, linenumber=True,
                                            diffopts=diffopts))
        for lineno, ((f, targetline), l) in lines:
            fnode = f.filenode()

            if last != fnode:
                last = fnode

            yield {"parity": parity.next(),
                   "node": f.hex(),
                   "rev": f.rev(),
                   "author": f.user(),
                   "desc": f.description(),
                   "extra": f.extra(),
                   "file": f.path(),
                   "targetline": targetline,
                   "line": l,
                   "lineid": "l%d" % (lineno + 1),
                   "linenumber": "% 6d" % (lineno + 1),
                   "revdate": f.date()}

    return tmpl("fileannotate",
                file=f,
                annotate=annotate,
                path=webutil.up(f),
                rev=fctx.rev(),
                node=fctx.hex(),
                author=fctx.user(),
                date=fctx.date(),
                desc=fctx.description(),
                extra=fctx.extra(),
                rename=webutil.renamelink(fctx),
                branch=webutil.nodebranchnodefault(fctx),
                parent=webutil.parents(fctx),
                child=webutil.children(fctx),
                permissions=fctx.manifest().flags(f))

def filelog(web, req, tmpl):

    try:
        fctx = webutil.filectx(web.repo, req)
        f = fctx.path()
        fl = fctx.filelog()
    except error.LookupError:
        f = webutil.cleanpath(web.repo, req.form['file'][0])
        fl = web.repo.file(f)
        numrevs = len(fl)
        if not numrevs: # file doesn't exist at all
            raise
        rev = webutil.changectx(web.repo, req).rev()
        first = fl.linkrev(0)
        if rev < first: # current rev is from before file existed
            raise
        frev = numrevs - 1
        while fl.linkrev(frev) > rev:
            frev -= 1
        fctx = web.repo.filectx(f, fl.linkrev(frev))

    revcount = web.maxshortchanges
    if 'revcount' in req.form:
        revcount = int(req.form.get('revcount', [revcount])[0])
        revcount = max(revcount, 1)
        tmpl.defaults['sessionvars']['revcount'] = revcount

    lessvars = copy.copy(tmpl.defaults['sessionvars'])
    lessvars['revcount'] = max(revcount / 2, 1)
    morevars = copy.copy(tmpl.defaults['sessionvars'])
    morevars['revcount'] = revcount * 2

    count = fctx.filerev() + 1
    start = max(0, fctx.filerev() - revcount + 1) # first rev on this page
    end = min(count, start + revcount) # last rev on this page
    parity = paritygen(web.stripecount, offset=start - end)

    def entries():
        l = []

        repo = web.repo
        revs = fctx.filelog().revs(start, end - 1)
        for i in revs:
            iterfctx = fctx.filectx(i)

            l.append({"parity": parity.next(),
                      "filerev": i,
                      "file": f,
                      "node": iterfctx.hex(),
                      "author": iterfctx.user(),
                      "date": iterfctx.date(),
                      "rename": webutil.renamelink(iterfctx),
                      "parent": webutil.parents(iterfctx),
                      "child": webutil.children(iterfctx),
                      "desc": iterfctx.description(),
                      "extra": iterfctx.extra(),
                      "tags": webutil.nodetagsdict(repo, iterfctx.node()),
                      "bookmarks": webutil.nodebookmarksdict(
                          repo, iterfctx.node()),
                      "branch": webutil.nodebranchnodefault(iterfctx),
                      "inbranch": webutil.nodeinbranch(repo, iterfctx),
                      "branches": webutil.nodebranchdict(repo, iterfctx)})
        for e in reversed(l):
            yield e

    entries = list(entries())
    latestentry = entries[:1]

    revnav = webutil.filerevnav(web.repo, fctx.path())
    nav = revnav.gen(end - 1, revcount, count)
    return tmpl("filelog", file=f, node=fctx.hex(), nav=nav,
                entries=entries,
                latestentry=latestentry,
                revcount=revcount, morevars=morevars, lessvars=lessvars)

def archive(web, req, tmpl):
    type_ = req.form.get('type', [None])[0]
    allowed = web.configlist("web", "allow_archive")
    key = req.form['node'][0]

    if type_ not in web.archives:
        msg = 'Unsupported archive type: %s' % type_
        raise ErrorResponse(HTTP_NOT_FOUND, msg)

    if not ((type_ in allowed or
        web.configbool("web", "allow" + type_, False))):
        msg = 'Archive type not allowed: %s' % type_
        raise ErrorResponse(HTTP_FORBIDDEN, msg)

    reponame = re.sub(r"\W+", "-", os.path.basename(web.reponame))
    cnode = web.repo.lookup(key)
    arch_version = key
    if cnode == key or key == 'tip':
        arch_version = short(cnode)
    name = "%s-%s" % (reponame, arch_version)

    ctx = webutil.changectx(web.repo, req)
    pats = []
    matchfn = None
    file = req.form.get('file', None)
    if file:
        pats = ['path:' + file[0]]
        matchfn = scmutil.match(ctx, pats, default='path')
        if pats:
            files = [f for f in ctx.manifest().keys() if matchfn(f)]
            if not files:
                raise ErrorResponse(HTTP_NOT_FOUND,
                    'file(s) not found: %s' % file[0])

    mimetype, artype, extension, encoding = web.archive_specs[type_]
    headers = [
        ('Content-Disposition', 'attachment; filename=%s%s' % (name, extension))
        ]
    if encoding:
        headers.append(('Content-Encoding', encoding))
    req.headers.extend(headers)
    req.respond(HTTP_OK, mimetype)

    archival.archive(web.repo, req, cnode, artype, prefix=name,
                     matchfn=matchfn,
                     subrepos=web.configbool("web", "archivesubrepos"))
    return []


def static(web, req, tmpl):
    fname = req.form['file'][0]
    # a repo owner may set web.static in .hg/hgrc to get any file
    # readable by the user running the CGI script
    static = web.config("web", "static", None, untrusted=False)
    if not static:
        tp = web.templatepath or templater.templatepath()
        if isinstance(tp, str):
            tp = [tp]
        static = [os.path.join(p, 'static') for p in tp]
    staticfile(static, fname, req)
    return []

def graph(web, req, tmpl):

    ctx = webutil.changectx(web.repo, req)
    rev = ctx.rev()

    bg_height = 39
    revcount = web.maxshortchanges
    if 'revcount' in req.form:
        revcount = int(req.form.get('revcount', [revcount])[0])
        revcount = max(revcount, 1)
        tmpl.defaults['sessionvars']['revcount'] = revcount

    lessvars = copy.copy(tmpl.defaults['sessionvars'])
    lessvars['revcount'] = max(revcount / 2, 1)
    morevars = copy.copy(tmpl.defaults['sessionvars'])
    morevars['revcount'] = revcount * 2

    count = len(web.repo)
    pos = rev

    uprev = min(max(0, count - 1), rev + revcount)
    downrev = max(0, rev - revcount)
    changenav = webutil.revnav(web.repo).gen(pos, revcount, count)

    tree = []
    if pos != -1:
        allrevs = web.repo.changelog.revs(pos, 0)
        revs = []
        for i in allrevs:
            revs.append(i)
            if len(revs) >= revcount:
                break

        dag = graphmod.dagwalker(web.repo, revs)
        tree = list(graphmod.colored(dag, web.repo))

    def getcolumns(tree):
        cols = 0
        for (id, type, ctx, vtx, edges) in tree:
            if type != graphmod.CHANGESET:
                continue
            cols = max(cols, max([edge[0] for edge in edges] or [0]),
                             max([edge[1] for edge in edges] or [0]))
        return cols

    def graphdata(usetuples, **map):
        data = []

        row = 0
        for (id, type, ctx, vtx, edges) in tree:
            if type != graphmod.CHANGESET:
                continue
            node = str(ctx)
            age = templatefilters.age(ctx.date())
            desc = templatefilters.firstline(ctx.description())
            desc = cgi.escape(templatefilters.nonempty(desc))
            user = cgi.escape(templatefilters.person(ctx.user()))
            branch = cgi.escape(ctx.branch())
            try:
                branchnode = web.repo.branchtip(branch)
            except error.RepoLookupError:
                branchnode = None
            branch = branch, branchnode == ctx.node()

            if usetuples:
                data.append((node, vtx, edges, desc, user, age, branch,
                             [cgi.escape(x) for x in ctx.tags()],
                             [cgi.escape(x) for x in ctx.bookmarks()]))
            else:
                edgedata = [dict(col=edge[0], nextcol=edge[1],
                                 color=(edge[2] - 1) % 6 + 1,
                                 width=edge[3], bcolor=edge[4])
                            for edge in edges]

                data.append(
                    dict(node=node,
                         col=vtx[0],
                         color=(vtx[1] - 1) % 6 + 1,
                         edges=edgedata,
                         row=row,
                         nextrow=row + 1,
                         desc=desc,
                         user=user,
                         age=age,
                         bookmarks=webutil.nodebookmarksdict(
                            web.repo, ctx.node()),
                         branches=webutil.nodebranchdict(web.repo, ctx),
                         inbranch=webutil.nodeinbranch(web.repo, ctx),
                         tags=webutil.nodetagsdict(web.repo, ctx.node())))

            row += 1

        return data

    cols = getcolumns(tree)
    rows = len(tree)
    canvasheight = (rows + 1) * bg_height - 27

    return tmpl('graph', rev=rev, revcount=revcount, uprev=uprev,
                lessvars=lessvars, morevars=morevars, downrev=downrev,
                cols=cols, rows=rows,
                canvaswidth=(cols + 1) * bg_height,
                truecanvasheight=rows * bg_height,
                canvasheight=canvasheight, bg_height=bg_height,
                jsdata=lambda **x: graphdata(True, **x),
                nodes=lambda **x: graphdata(False, **x),
                node=ctx.hex(), changenav=changenav)

def _getdoc(e):
    doc = e[0].__doc__
    if doc:
        doc = _(doc).split('\n')[0]
    else:
        doc = _('(no help text available)')
    return doc

def help(web, req, tmpl):
    from mercurial import commands # avoid cycle

    topicname = req.form.get('node', [None])[0]
    if not topicname:
        def topics(**map):
            for entries, summary, _ in helpmod.helptable:
                yield {'topic': entries[0], 'summary': summary}

        early, other = [], []
        primary = lambda s: s.split('|')[0]
        for c, e in commands.table.iteritems():
            doc = _getdoc(e)
            if 'DEPRECATED' in doc or c.startswith('debug'):
                continue
            cmd = primary(c)
            if cmd.startswith('^'):
                early.append((cmd[1:], doc))
            else:
                other.append((cmd, doc))

        early.sort()
        other.sort()

        def earlycommands(**map):
            for c, doc in early:
                yield {'topic': c, 'summary': doc}

        def othercommands(**map):
            for c, doc in other:
                yield {'topic': c, 'summary': doc}

        return tmpl('helptopics', topics=topics, earlycommands=earlycommands,
                    othercommands=othercommands, title='Index')

    u = webutil.wsgiui()
    u.verbose = True
    try:
        doc = helpmod.help_(u, topicname)
    except error.UnknownCommand:
        raise ErrorResponse(HTTP_NOT_FOUND)
    return tmpl('help', topic=topicname, doc=doc)
