#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import cgi
import copy
import mimetypes
import os
import re

from ..i18n import _
from ..node import hex, short

from .common import (
    ErrorResponse,
    HTTP_FORBIDDEN,
    HTTP_NOT_FOUND,
    HTTP_OK,
    get_contact,
    paritygen,
    staticfile,
)

from .. import (
    archival,
    encoding,
    error,
    graphmod,
    patch,
    revset,
    scmutil,
    templatefilters,
    templater,
    util,
)

from . import (
    webutil,
)

__all__ = []
commands = {}

class webcommand(object):
    """Decorator used to register a web command handler.

    The decorator takes as its positional arguments the name/path the
    command should be accessible under.

    Usage:

    @webcommand('mycommand')
    def mycommand(web, req, tmpl):
        pass
    """

    def __init__(self, name):
        self.name = name

    def __call__(self, func):
        __all__.append(self.name)
        commands[self.name] = func
        return func

@webcommand('log')
def log(web, req, tmpl):
    """
    /log[/{revision}[/{path}]]
    --------------------------

    Show repository or file history.

    For URLs of the form ``/log/{revision}``, a list of changesets starting at
    the specified changeset identifier is shown. If ``{revision}`` is not
    defined, the default is ``tip``. This form is equivalent to the
    ``changelog`` handler.

    For URLs of the form ``/log/{revision}/{file}``, the history for a specific
    file will be shown. This form is equivalent to the ``filelog`` handler.
    """

    if 'file' in req.form and req.form['file'][0]:
        return filelog(web, req, tmpl)
    else:
        return changelog(web, req, tmpl)

@webcommand('rawfile')
def rawfile(web, req, tmpl):
    guessmime = web.configbool('web', 'guessmime', False)

    path = webutil.cleanpath(web.repo, req.form.get('file', [''])[0])
    if not path:
        content = manifest(web, req, tmpl)
        req.respond(HTTP_OK, web.ctype)
        return content

    try:
        fctx = webutil.filectx(web.repo, req)
    except error.LookupError as inst:
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
            if util.binary(text):
                mt = 'application/binary'
            else:
                mt = 'text/plain'
    if mt.startswith('text/'):
        mt += '; charset="%s"' % encoding.encoding

    req.respond(HTTP_OK, mt, path, body=text)
    return []

def _filerevision(web, req, tmpl, fctx):
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
                symrev=webutil.symrevorshortnode(req, fctx),
                rename=webutil.renamelink(fctx),
                permissions=fctx.manifest().flags(f),
                **webutil.commonentry(web.repo, fctx))

@webcommand('file')
def file(web, req, tmpl):
    """
    /file/{revision}[/{path}]
    -------------------------

    Show information about a directory or file in the repository.

    Info about the ``path`` given as a URL parameter will be rendered.

    If ``path`` is a directory, information about the entries in that
    directory will be rendered. This form is equivalent to the ``manifest``
    handler.

    If ``path`` is a file, information about that file will be shown via
    the ``filerevision`` template.

    If ``path`` is not defined, information about the root directory will
    be rendered.
    """
    path = webutil.cleanpath(web.repo, req.form.get('file', [''])[0])
    if not path:
        return manifest(web, req, tmpl)
    try:
        return _filerevision(web, req, tmpl, webutil.filectx(web.repo, req))
    except error.LookupError as inst:
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
            tree = revset.parse(revdef)
        except error.ParseError:
            # can't parse to a revset tree
            return MODE_KEYWORD, query

        if revset.depth(tree) <= 2:
            # no revset syntax used
            return MODE_KEYWORD, query

        if any((token, (value or '')[:3]) == ('string', 're:')
                    for token, value, pos in revset.tokenize(revdef)):
            return MODE_KEYWORD, query

        funcsused = revset.funcsused(tree)
        if not funcsused.issubset(revset.safesymbols):
            return MODE_KEYWORD, query

        mfunc = revset.match(web.repo.ui, revdef)
        try:
            revs = mfunc(web.repo)
            return MODE_REVSET, revs
            # ParseError: wrongly placed tokens, wrongs arguments, etc
            # RepoLookupError: no such revision, e.g. in 'revision:'
            # Abort: bookmark/tag not exists
            # LookupError: ambiguous identifier, e.g. in '(bc)' on a large repo
        except (error.ParseError, error.RepoLookupError, error.Abort,
                LookupError):
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
                       changelogtag=showtags,
                       files=files,
                       **webutil.commonentry(web.repo, ctx))

            if count >= revcount:
                break

    query = req.form['rev'][0]
    revcount = web.maxchanges
    if 'revcount' in req.form:
        try:
            revcount = int(req.form.get('revcount', [revcount])[0])
            revcount = max(revcount, 1)
            tmpl.defaults['sessionvars']['revcount'] = revcount
        except ValueError:
            pass

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

    return tmpl('search', query=query, node=tip.hex(), symrev='tip',
                entries=changelist, archives=web.archivelist("tip"),
                morevars=morevars, lessvars=lessvars,
                modedesc=searchfunc[1],
                showforcekw=showforcekw, showunforcekw=showunforcekw)

@webcommand('changelog')
def changelog(web, req, tmpl, shortlog=False):
    """
    /changelog[/{revision}]
    -----------------------

    Show information about multiple changesets.

    If the optional ``revision`` URL argument is absent, information about
    all changesets starting at ``tip`` will be rendered. If the ``revision``
    argument is present, changesets will be shown starting from the specified
    revision.

    If ``revision`` is absent, the ``rev`` query string argument may be
    defined. This will perform a search for changesets.

    The argument for ``rev`` can be a single revision, a revision set,
    or a literal keyword to search for in changeset data (equivalent to
    :hg:`log -k`).

    The ``revcount`` query string argument defines the maximum numbers of
    changesets to render.

    For non-searches, the ``changelog`` template will be rendered.
    """

    query = ''
    if 'node' in req.form:
        ctx = webutil.changectx(web.repo, req)
        symrev = webutil.symrevorshortnode(req, ctx)
    elif 'rev' in req.form:
        return _search(web, req, tmpl)
    else:
        ctx = web.repo['tip']
        symrev = 'tip'

    def changelist():
        revs = []
        if pos != -1:
            revs = web.repo.changelog.revs(pos, 0)
        curcount = 0
        for rev in revs:
            curcount += 1
            if curcount > revcount + 1:
                break

            entry = webutil.changelistentry(web, web.repo[rev], tmpl)
            entry['parity'] = parity.next()
            yield entry

    if shortlog:
        revcount = web.maxshortchanges
    else:
        revcount = web.maxchanges

    if 'revcount' in req.form:
        try:
            revcount = int(req.form.get('revcount', [revcount])[0])
            revcount = max(revcount, 1)
            tmpl.defaults['sessionvars']['revcount'] = revcount
        except ValueError:
            pass

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
                node=ctx.hex(), rev=pos, symrev=symrev, changesets=count,
                entries=entries,
                latestentry=latestentry, nextentry=nextentry,
                archives=web.archivelist("tip"), revcount=revcount,
                morevars=morevars, lessvars=lessvars, query=query)

@webcommand('shortlog')
def shortlog(web, req, tmpl):
    """
    /shortlog
    ---------

    Show basic information about a set of changesets.

    This accepts the same parameters as the ``changelog`` handler. The only
    difference is the ``shortlog`` template will be rendered instead of the
    ``changelog`` template.
    """
    return changelog(web, req, tmpl, shortlog=True)

@webcommand('changeset')
def changeset(web, req, tmpl):
    """
    /changeset[/{revision}]
    -----------------------

    Show information about a single changeset.

    A URL path argument is the changeset identifier to show. See ``hg help
    revisions`` for possible values. If not defined, the ``tip`` changeset
    will be shown.

    The ``changeset`` template is rendered. Contents of the ``changesettag``,
    ``changesetbookmark``, ``filenodelink``, ``filenolink``, and the many
    templates related to diffs may all be used to produce the output.
    """
    ctx = webutil.changectx(web.repo, req)

    return tmpl('changeset', **webutil.changesetentry(web, req, tmpl, ctx))

rev = webcommand('rev')(changeset)

def decodepath(path):
    """Hook for mapping a path in the repository to a path in the
    working copy.

    Extensions (e.g., largefiles) can override this to remap files in
    the virtual file system presented by the manifest command below."""
    return path

@webcommand('manifest')
def manifest(web, req, tmpl):
    """
    /manifest[/{revision}[/{path}]]
    -------------------------------

    Show information about a directory.

    If the URL path arguments are omitted, information about the root
    directory for the ``tip`` changeset will be shown.

    Because this handler can only show information for directories, it
    is recommended to use the ``file`` handler instead, as it can handle both
    directories and files.

    The ``manifest`` template will be rendered for this handler.
    """
    if 'node' in req.form:
        ctx = webutil.changectx(web.repo, req)
        symrev = webutil.symrevorshortnode(req, ctx)
    else:
        ctx = web.repo['tip']
        symrev = 'tip'
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
                symrev=symrev,
                path=abspath,
                up=webutil.up(abspath),
                upparity=parity.next(),
                fentries=filelist,
                dentries=dirlist,
                archives=web.archivelist(hex(node)),
                **webutil.commonentry(web.repo, ctx))

@webcommand('tags')
def tags(web, req, tmpl):
    """
    /tags
    -----

    Show information about tags.

    No arguments are accepted.

    The ``tags`` template is rendered.
    """
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

@webcommand('bookmarks')
def bookmarks(web, req, tmpl):
    """
    /bookmarks
    ----------

    Show information about bookmarks.

    No arguments are accepted.

    The ``bookmarks`` template is rendered.
    """
    i = [b for b in web.repo._bookmarks.items() if b[1] in web.repo]
    sortkey = lambda b: (web.repo[b[1]].rev(), b[0])
    i = sorted(i, key=sortkey, reverse=True)
    parity = paritygen(web.stripecount)

    def entries(latestonly, **map):
        t = i
        if latestonly:
            t = i[:1]
        for k, n in t:
            yield {"parity": parity.next(),
                   "bookmark": k,
                   "date": web.repo[n].date(),
                   "node": hex(n)}

    if i:
        latestrev = i[0][1]
    else:
        latestrev = -1

    return tmpl("bookmarks",
                node=hex(web.repo.changelog.tip()),
                lastchange=[{"date": web.repo[latestrev].date()}],
                entries=lambda **x: entries(latestonly=False, **x),
                latestentry=lambda **x: entries(latestonly=True, **x))

@webcommand('branches')
def branches(web, req, tmpl):
    """
    /branches
    ---------

    Show information about branches.

    All known branches are contained in the output, even closed branches.

    No arguments are accepted.

    The ``branches`` template is rendered.
    """
    entries = webutil.branchentries(web.repo, web.stripecount)
    latestentry = webutil.branchentries(web.repo, web.stripecount, 1)
    return tmpl('branches', node=hex(web.repo.changelog.tip()),
                entries=entries, latestentry=latestentry)

@webcommand('summary')
def summary(web, req, tmpl):
    """
    /summary
    --------

    Show a summary of repository state.

    Information about the latest changesets, bookmarks, tags, and branches
    is captured by this handler.

    The ``summary`` template is rendered.
    """
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
        sortkey = lambda b: (web.repo[b[1]].rev(), b[0])
        marks = sorted(marks, key=sortkey, reverse=True)
        for k, n in marks[:10]:  # limit to 10 bookmarks
            yield {'parity': parity.next(),
                   'bookmark': k,
                   'date': web.repo[n].date(),
                   'node': hex(n)}

    def changelist(**map):
        parity = paritygen(web.stripecount, offset=start - end)
        l = [] # build a list in forward order for efficiency
        revs = []
        if start < end:
            revs = web.repo.changelog.revs(start, end - 1)
        for i in revs:
            ctx = web.repo[i]

            l.append(tmpl(
                'shortlogentry',
                parity=parity.next(),
                **webutil.commonentry(web.repo, ctx)))

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
                branches=webutil.branchentries(web.repo, web.stripecount, 10),
                shortlog=changelist,
                node=tip.hex(),
                symrev='tip',
                archives=web.archivelist("tip"))

@webcommand('filediff')
def filediff(web, req, tmpl):
    """
    /diff/{revision}/{path}
    -----------------------

    Show how a file changed in a particular commit.

    The ``filediff`` template is rendered.

    This handler is registered under both the ``/diff`` and ``/filediff``
    paths. ``/diff`` is used in modern code.
    """
    fctx, ctx = None, None
    try:
        fctx = webutil.filectx(web.repo, req)
    except LookupError:
        ctx = webutil.changectx(web.repo, req)
        path = webutil.cleanpath(web.repo, req.form['file'][0])
        if path not in ctx.files():
            raise

    if fctx is not None:
        path = fctx.path()
        ctx = fctx.changectx()

    parity = paritygen(web.stripecount)
    style = web.config('web', 'style', 'paper')
    if 'style' in req.form:
        style = req.form['style'][0]

    diffs = webutil.diffs(web.repo, tmpl, ctx, None, [path], parity, style)
    if fctx is not None:
        rename = webutil.renamelink(fctx)
        ctx = fctx
    else:
        rename = []
        ctx = ctx
    return tmpl("filediff",
                file=path,
                symrev=webutil.symrevorshortnode(req, ctx),
                rename=rename,
                diff=diffs,
                **webutil.commonentry(web.repo, ctx))

diff = webcommand('diff')(filediff)

@webcommand('comparison')
def comparison(web, req, tmpl):
    """
    /comparison/{revision}/{path}
    -----------------------------

    Show a comparison between the old and new versions of a file from changes
    made on a particular revision.

    This is similar to the ``diff`` handler. However, this form features
    a split or side-by-side diff rather than a unified diff.

    The ``context`` query string argument can be used to control the lines of
    context in the diff.

    The ``filecomparison`` template is rendered.
    """
    ctx = webutil.changectx(web.repo, req)
    if 'file' not in req.form:
        raise ErrorResponse(HTTP_NOT_FOUND, 'file not given')
    path = webutil.cleanpath(web.repo, req.form['file'][0])

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

    fctx = None
    parent = ctx.p1()
    leftrev = parent.rev()
    leftnode = parent.node()
    rightrev = ctx.rev()
    rightnode = ctx.node()
    if path in ctx:
        fctx = ctx[path]
        rightlines = filelines(fctx)
        if path not in parent:
            leftlines = ()
        else:
            pfctx = parent[path]
            leftlines = filelines(pfctx)
    else:
        rightlines = ()
        pfctx = ctx.parents()[0][path]
        leftlines = filelines(pfctx)

    comparison = webutil.compare(tmpl, context, leftlines, rightlines)
    if fctx is not None:
        rename = webutil.renamelink(fctx)
        ctx = fctx
    else:
        rename = []
        ctx = ctx
    return tmpl('filecomparison',
                file=path,
                symrev=webutil.symrevorshortnode(req, ctx),
                rename=rename,
                leftrev=leftrev,
                leftnode=hex(leftnode),
                rightrev=rightrev,
                rightnode=hex(rightnode),
                comparison=comparison,
                **webutil.commonentry(web.repo, ctx))

@webcommand('annotate')
def annotate(web, req, tmpl):
    """
    /annotate/{revision}/{path}
    ---------------------------

    Show changeset information for each line in a file.

    The ``fileannotate`` template is rendered.
    """
    fctx = webutil.filectx(web.repo, req)
    f = fctx.path()
    parity = paritygen(web.stripecount)
    diffopts = patch.difffeatureopts(web.repo.ui, untrusted=True,
                                     section='annotate', whitespace=True)

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
                   "lineno": lineno + 1,
                   "lineid": "l%d" % (lineno + 1),
                   "linenumber": "% 6d" % (lineno + 1),
                   "revdate": f.date()}

    return tmpl("fileannotate",
                file=f,
                annotate=annotate,
                path=webutil.up(f),
                symrev=webutil.symrevorshortnode(req, fctx),
                rename=webutil.renamelink(fctx),
                permissions=fctx.manifest().flags(f),
                **webutil.commonentry(web.repo, fctx))

@webcommand('filelog')
def filelog(web, req, tmpl):
    """
    /filelog/{revision}/{path}
    --------------------------

    Show information about the history of a file in the repository.

    The ``revcount`` query string argument can be defined to control the
    maximum number of entries to show.

    The ``filelog`` template will be rendered.
    """

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
        try:
            revcount = int(req.form.get('revcount', [revcount])[0])
            revcount = max(revcount, 1)
            tmpl.defaults['sessionvars']['revcount'] = revcount
        except ValueError:
            pass

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

            l.append(dict(
                parity=parity.next(),
                filerev=i,
                file=f,
                rename=webutil.renamelink(iterfctx),
                **webutil.commonentry(repo, iterfctx)))
        for e in reversed(l):
            yield e

    entries = list(entries())
    latestentry = entries[:1]

    revnav = webutil.filerevnav(web.repo, fctx.path())
    nav = revnav.gen(end - 1, revcount, count)
    return tmpl("filelog",
                file=f,
                nav=nav,
                symrev=webutil.symrevorshortnode(req, fctx),
                entries=entries,
                latestentry=latestentry,
                revcount=revcount,
                morevars=morevars,
                lessvars=lessvars,
                **webutil.commonentry(web.repo, fctx))

@webcommand('archive')
def archive(web, req, tmpl):
    """
    /archive/{revision}.{format}[/{path}]
    -------------------------------------

    Obtain an archive of repository content.

    The content and type of the archive is defined by a URL path parameter.
    ``format`` is the file extension of the archive type to be generated. e.g.
    ``zip`` or ``tar.bz2``. Not all archive types may be allowed by your
    server configuration.

    The optional ``path`` URL parameter controls content to include in the
    archive. If omitted, every file in the specified revision is present in the
    archive. If included, only the specified file or contents of the specified
    directory will be included in the archive.

    No template is used for this handler. Raw, binary content is generated.
    """

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
    matchfn = scmutil.match(ctx, [])
    file = req.form.get('file', None)
    if file:
        pats = ['path:' + file[0]]
        matchfn = scmutil.match(ctx, pats, default='path')
        if pats:
            files = [f for f in ctx.manifest().keys() if matchfn(f)]
            if not files:
                raise ErrorResponse(HTTP_NOT_FOUND,
                    'file(s) not found: %s' % file[0])

    mimetype, artype, extension, encoding = web.archivespecs[type_]
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


@webcommand('static')
def static(web, req, tmpl):
    fname = req.form['file'][0]
    # a repo owner may set web.static in .hg/hgrc to get any file
    # readable by the user running the CGI script
    static = web.config("web", "static", None, untrusted=False)
    if not static:
        tp = web.templatepath or templater.templatepaths()
        if isinstance(tp, str):
            tp = [tp]
        static = [os.path.join(p, 'static') for p in tp]
    staticfile(static, fname, req)
    return []

@webcommand('graph')
def graph(web, req, tmpl):
    """
    /graph[/{revision}]
    -------------------

    Show information about the graphical topology of the repository.

    Information rendered by this handler can be used to create visual
    representations of repository topology.

    The ``revision`` URL parameter controls the starting changeset.

    The ``revcount`` query string argument can define the number of changesets
    to show information for.

    This handler will render the ``graph`` template.
    """

    if 'node' in req.form:
        ctx = webutil.changectx(web.repo, req)
        symrev = webutil.symrevorshortnode(req, ctx)
    else:
        ctx = web.repo['tip']
        symrev = 'tip'
    rev = ctx.rev()

    bg_height = 39
    revcount = web.maxshortchanges
    if 'revcount' in req.form:
        try:
            revcount = int(req.form.get('revcount', [revcount])[0])
            revcount = max(revcount, 1)
            tmpl.defaults['sessionvars']['revcount'] = revcount
        except ValueError:
            pass

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

        # We have to feed a baseset to dagwalker as it is expecting smartset
        # object. This does not have a big impact on hgweb performance itself
        # since hgweb graphing code is not itself lazy yet.
        dag = graphmod.dagwalker(web.repo, revset.baseset(revs))
        # As we said one line above... not lazy.
        tree = list(graphmod.colored(dag, web.repo))

    def getcolumns(tree):
        cols = 0
        for (id, type, ctx, vtx, edges) in tree:
            if type != graphmod.CHANGESET:
                continue
            cols = max(cols, max([edge[0] for edge in edges] or [0]),
                             max([edge[1] for edge in edges] or [0]))
        return cols

    def graphdata(usetuples, encodestr):
        data = []

        row = 0
        for (id, type, ctx, vtx, edges) in tree:
            if type != graphmod.CHANGESET:
                continue
            node = str(ctx)
            age = encodestr(templatefilters.age(ctx.date()))
            desc = templatefilters.firstline(encodestr(ctx.description()))
            desc = cgi.escape(templatefilters.nonempty(desc))
            user = cgi.escape(templatefilters.person(encodestr(ctx.user())))
            branch = cgi.escape(encodestr(ctx.branch()))
            try:
                branchnode = web.repo.branchtip(branch)
            except error.RepoLookupError:
                branchnode = None
            branch = branch, branchnode == ctx.node()

            if usetuples:
                data.append((node, vtx, edges, desc, user, age, branch,
                             [cgi.escape(encodestr(x)) for x in ctx.tags()],
                             [cgi.escape(encodestr(x))
                              for x in ctx.bookmarks()]))
            else:
                edgedata = [{'col': edge[0], 'nextcol': edge[1],
                             'color': (edge[2] - 1) % 6 + 1,
                             'width': edge[3], 'bcolor': edge[4]}
                            for edge in edges]

                data.append(
                    {'node': node,
                     'col': vtx[0],
                     'color': (vtx[1] - 1) % 6 + 1,
                     'edges': edgedata,
                     'row': row,
                     'nextrow': row + 1,
                     'desc': desc,
                     'user': user,
                     'age': age,
                     'bookmarks': webutil.nodebookmarksdict(
                         web.repo, ctx.node()),
                     'branches': webutil.nodebranchdict(web.repo, ctx),
                     'inbranch': webutil.nodeinbranch(web.repo, ctx),
                     'tags': webutil.nodetagsdict(web.repo, ctx.node())})

            row += 1

        return data

    cols = getcolumns(tree)
    rows = len(tree)
    canvasheight = (rows + 1) * bg_height - 27

    return tmpl('graph', rev=rev, symrev=symrev, revcount=revcount,
                uprev=uprev,
                lessvars=lessvars, morevars=morevars, downrev=downrev,
                cols=cols, rows=rows,
                canvaswidth=(cols + 1) * bg_height,
                truecanvasheight=rows * bg_height,
                canvasheight=canvasheight, bg_height=bg_height,
                # {jsdata} will be passed to |json, so it must be in utf-8
                jsdata=lambda **x: graphdata(True, encoding.fromlocal),
                nodes=lambda **x: graphdata(False, str),
                node=ctx.hex(), changenav=changenav)

def _getdoc(e):
    doc = e[0].__doc__
    if doc:
        doc = _(doc).partition('\n')[0]
    else:
        doc = _('(no help text available)')
    return doc

@webcommand('help')
def help(web, req, tmpl):
    """
    /help[/{topic}]
    ---------------

    Render help documentation.

    This web command is roughly equivalent to :hg:`help`. If a ``topic``
    is defined, that help topic will be rendered. If not, an index of
    available help topics will be rendered.

    The ``help`` template will be rendered when requesting help for a topic.
    ``helptopics`` will be rendered for the index of help topics.
    """
    from .. import commands, help as helpmod  # avoid cycle

    topicname = req.form.get('node', [None])[0]
    if not topicname:
        def topics(**map):
            for entries, summary, _doc in helpmod.helptable:
                yield {'topic': entries[0], 'summary': summary}

        early, other = [], []
        primary = lambda s: s.partition('|')[0]
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

    # Render an index of sub-topics.
    if topicname in helpmod.subtopics:
        topics = []
        for entries, summary, _doc in helpmod.subtopics[topicname]:
            topics.append({
                'topic': '%s.%s' % (topicname, entries[0]),
                'basename': entries[0],
                'summary': summary,
            })

        return tmpl('helptopics', topics=topics, title=topicname,
                    subindex=True)

    u = webutil.wsgiui()
    u.verbose = True

    # Render a page from a sub-topic.
    if '.' in topicname:
        # TODO implement support for rendering sections, like
        # `hg help` works.
        topic, subtopic = topicname.split('.', 1)
        if topic not in helpmod.subtopics:
            raise ErrorResponse(HTTP_NOT_FOUND)
    else:
        topic = topicname
        subtopic = None

    try:
        doc = helpmod.help_(u, topic, subtopic=subtopic)
    except error.UnknownCommand:
        raise ErrorResponse(HTTP_NOT_FOUND)
    return tmpl('help', topic=topicname, doc=doc)

# tell hggettext to extract docstrings from these functions:
i18nfunctions = commands.values()
