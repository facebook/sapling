# templatekw.py - common changeset template keywords
#
# Copyright 2005-2009 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .node import hex, nullid
from . import (
    encoding,
    error,
    hbisect,
    patch,
    registrar,
    scmutil,
    util,
)

# This helper class allows us to handle both:
#  "{files}" (legacy command-line-specific list hack) and
#  "{files % '{file}\n'}" (hgweb-style with inlining and function support)
# and to access raw values:
#  "{ifcontains(file, files, ...)}", "{ifcontains(key, extras, ...)}"
#  "{get(extras, key)}"

class _hybrid(object):
    def __init__(self, gen, values, makemap, joinfmt=None):
        self.gen = gen
        self.values = values
        self._makemap = makemap
        if joinfmt:
            self.joinfmt = joinfmt
        else:
            self.joinfmt = lambda x: x.values()[0]
    def __iter__(self):
        return self.gen
    def itermaps(self):
        makemap = self._makemap
        for x in self.values:
            yield makemap(x)
    def __contains__(self, x):
        return x in self.values
    def __len__(self):
        return len(self.values)
    def __getattr__(self, name):
        if name != 'get':
            raise AttributeError(name)
        return getattr(self.values, name)

def showlist(name, values, plural=None, element=None, separator=' ', **args):
    if not element:
        element = name
    f = _showlist(name, values, plural, separator, **args)
    return _hybrid(f, values, lambda x: {element: x})

def _showlist(name, values, plural=None, separator=' ', **args):
    '''expand set of values.
    name is name of key in template map.
    values is list of strings or dicts.
    plural is plural of name, if not simply name + 's'.
    separator is used to join values as a string

    expansion works like this, given name 'foo'.

    if values is empty, expand 'no_foos'.

    if 'foo' not in template map, return values as a string,
    joined by 'separator'.

    expand 'start_foos'.

    for each value, expand 'foo'. if 'last_foo' in template
    map, expand it instead of 'foo' for last key.

    expand 'end_foos'.
    '''
    templ = args['templ']
    if plural:
        names = plural
    else: names = name + 's'
    if not values:
        noname = 'no_' + names
        if noname in templ:
            yield templ(noname, **args)
        return
    if name not in templ:
        if isinstance(values[0], str):
            yield separator.join(values)
        else:
            for v in values:
                yield dict(v, **args)
        return
    startname = 'start_' + names
    if startname in templ:
        yield templ(startname, **args)
    vargs = args.copy()
    def one(v, tag=name):
        try:
            vargs.update(v)
        except (AttributeError, ValueError):
            try:
                for a, b in v:
                    vargs[a] = b
            except ValueError:
                vargs[name] = v
        return templ(tag, **vargs)
    lastname = 'last_' + name
    if lastname in templ:
        last = values.pop()
    else:
        last = None
    for v in values:
        yield one(v)
    if last is not None:
        yield one(last, tag=lastname)
    endname = 'end_' + names
    if endname in templ:
        yield templ(endname, **args)

def getfiles(repo, ctx, revcache):
    if 'files' not in revcache:
        revcache['files'] = repo.status(ctx.p1(), ctx)[:3]
    return revcache['files']

def getlatesttags(repo, ctx, cache, pattern=None):
    '''return date, distance and name for the latest tag of rev'''

    cachename = 'latesttags'
    if pattern is not None:
        cachename += '-' + pattern
        match = util.stringmatcher(pattern)[2]
    else:
        match = util.always

    if cachename not in cache:
        # Cache mapping from rev to a tuple with tag date, tag
        # distance and tag name
        cache[cachename] = {-1: (0, 0, ['null'])}
    latesttags = cache[cachename]

    rev = ctx.rev()
    todo = [rev]
    while todo:
        rev = todo.pop()
        if rev in latesttags:
            continue
        ctx = repo[rev]
        tags = [t for t in ctx.tags()
                if (repo.tagtype(t) and repo.tagtype(t) != 'local'
                    and match(t))]
        if tags:
            latesttags[rev] = ctx.date()[0], 0, [t for t in sorted(tags)]
            continue
        try:
            # The tuples are laid out so the right one can be found by
            # comparison.
            pdate, pdist, ptag = max(
                latesttags[p.rev()] for p in ctx.parents())
        except KeyError:
            # Cache miss - recurse
            todo.append(rev)
            todo.extend(p.rev() for p in ctx.parents())
            continue
        latesttags[rev] = pdate, pdist + 1, ptag
    return latesttags[rev]

def getrenamedfn(repo, endrev=None):
    rcache = {}
    if endrev is None:
        endrev = len(repo)

    def getrenamed(fn, rev):
        '''looks up all renames for a file (up to endrev) the first
        time the file is given. It indexes on the changerev and only
        parses the manifest if linkrev != changerev.
        Returns rename info for fn at changerev rev.'''
        if fn not in rcache:
            rcache[fn] = {}
            fl = repo.file(fn)
            for i in fl:
                lr = fl.linkrev(i)
                renamed = fl.renamed(fl.node(i))
                rcache[fn][lr] = renamed
                if lr >= endrev:
                    break
        if rev in rcache[fn]:
            return rcache[fn][rev]

        # If linkrev != rev (i.e. rev not found in rcache) fallback to
        # filectx logic.
        try:
            return repo[rev][fn].renamed()
        except error.LookupError:
            return None

    return getrenamed

# keywords are callables like:
# fn(repo, ctx, templ, cache, revcache, **args)
# with:
# repo - current repository instance
# ctx - the changectx being displayed
# templ - the templater instance
# cache - a cache dictionary for the whole templater run
# revcache - a cache dictionary for the current revision
keywords = {}

templatekeyword = registrar.templatekeyword(keywords)

@templatekeyword('author')
def showauthor(repo, ctx, templ, **args):
    """String. The unmodified author of the changeset."""
    return ctx.user()

@templatekeyword('bisect')
def showbisect(repo, ctx, templ, **args):
    """String. The changeset bisection status."""
    return hbisect.label(repo, ctx.node())

@templatekeyword('branch')
def showbranch(**args):
    """String. The name of the branch on which the changeset was
    committed.
    """
    return args['ctx'].branch()

@templatekeyword('branches')
def showbranches(**args):
    """List of strings. The name of the branch on which the
    changeset was committed. Will be empty if the branch name was
    default. (DEPRECATED)
    """
    branch = args['ctx'].branch()
    if branch != 'default':
        return showlist('branch', [branch], plural='branches', **args)
    return showlist('branch', [], plural='branches', **args)

@templatekeyword('bookmarks')
def showbookmarks(**args):
    """List of strings. Any bookmarks associated with the
    changeset. Also sets 'active', the name of the active bookmark.
    """
    repo = args['ctx']._repo
    bookmarks = args['ctx'].bookmarks()
    active = repo._activebookmark
    makemap = lambda v: {'bookmark': v, 'active': active, 'current': active}
    f = _showlist('bookmark', bookmarks, **args)
    return _hybrid(f, bookmarks, makemap, lambda x: x['bookmark'])

@templatekeyword('children')
def showchildren(**args):
    """List of strings. The children of the changeset."""
    ctx = args['ctx']
    childrevs = ['%d:%s' % (cctx, cctx) for cctx in ctx.children()]
    return showlist('children', childrevs, element='child', **args)

# Deprecated, but kept alive for help generation a purpose.
@templatekeyword('currentbookmark')
def showcurrentbookmark(**args):
    """String. The active bookmark, if it is
    associated with the changeset (DEPRECATED)"""
    return showactivebookmark(**args)

@templatekeyword('activebookmark')
def showactivebookmark(**args):
    """String. The active bookmark, if it is
    associated with the changeset"""
    active = args['repo']._activebookmark
    if active and active in args['ctx'].bookmarks():
        return active
    return ''

@templatekeyword('date')
def showdate(repo, ctx, templ, **args):
    """Date information. The date when the changeset was committed."""
    return ctx.date()

@templatekeyword('desc')
def showdescription(repo, ctx, templ, **args):
    """String. The text of the changeset description."""
    s = ctx.description()
    if isinstance(s, encoding.localstr):
        # try hard to preserve utf-8 bytes
        return encoding.tolocal(encoding.fromlocal(s).strip())
    else:
        return s.strip()

@templatekeyword('diffstat')
def showdiffstat(repo, ctx, templ, **args):
    """String. Statistics of changes with the following format:
    "modified files: +added/-removed lines"
    """
    stats = patch.diffstatdata(util.iterlines(ctx.diff()))
    maxname, maxtotal, adds, removes, binary = patch.diffstatsum(stats)
    return '%s: +%s/-%s' % (len(stats), adds, removes)

@templatekeyword('extras')
def showextras(**args):
    """List of dicts with key, value entries of the 'extras'
    field of this changeset."""
    extras = args['ctx'].extra()
    extras = util.sortdict((k, extras[k]) for k in sorted(extras))
    makemap = lambda k: {'key': k, 'value': extras[k]}
    c = [makemap(k) for k in extras]
    f = _showlist('extra', c, plural='extras', **args)
    return _hybrid(f, extras, makemap,
                   lambda x: '%s=%s' % (x['key'], x['value']))

@templatekeyword('file_adds')
def showfileadds(**args):
    """List of strings. Files added by this changeset."""
    repo, ctx, revcache = args['repo'], args['ctx'], args['revcache']
    return showlist('file_add', getfiles(repo, ctx, revcache)[1],
                    element='file', **args)

@templatekeyword('file_copies')
def showfilecopies(**args):
    """List of strings. Files copied in this changeset with
    their sources.
    """
    cache, ctx = args['cache'], args['ctx']
    copies = args['revcache'].get('copies')
    if copies is None:
        if 'getrenamed' not in cache:
            cache['getrenamed'] = getrenamedfn(args['repo'])
        copies = []
        getrenamed = cache['getrenamed']
        for fn in ctx.files():
            rename = getrenamed(fn, ctx.rev())
            if rename:
                copies.append((fn, rename[0]))

    copies = util.sortdict(copies)
    makemap = lambda k: {'name': k, 'source': copies[k]}
    c = [makemap(k) for k in copies]
    f = _showlist('file_copy', c, plural='file_copies', **args)
    return _hybrid(f, copies, makemap,
                   lambda x: '%s (%s)' % (x['name'], x['source']))

# showfilecopiesswitch() displays file copies only if copy records are
# provided before calling the templater, usually with a --copies
# command line switch.
@templatekeyword('file_copies_switch')
def showfilecopiesswitch(**args):
    """List of strings. Like "file_copies" but displayed
    only if the --copied switch is set.
    """
    copies = args['revcache'].get('copies') or []
    copies = util.sortdict(copies)
    makemap = lambda k: {'name': k, 'source': copies[k]}
    c = [makemap(k) for k in copies]
    f = _showlist('file_copy', c, plural='file_copies', **args)
    return _hybrid(f, copies, makemap,
                   lambda x: '%s (%s)' % (x['name'], x['source']))

@templatekeyword('file_dels')
def showfiledels(**args):
    """List of strings. Files removed by this changeset."""
    repo, ctx, revcache = args['repo'], args['ctx'], args['revcache']
    return showlist('file_del', getfiles(repo, ctx, revcache)[2],
                    element='file', **args)

@templatekeyword('file_mods')
def showfilemods(**args):
    """List of strings. Files modified by this changeset."""
    repo, ctx, revcache = args['repo'], args['ctx'], args['revcache']
    return showlist('file_mod', getfiles(repo, ctx, revcache)[0],
                    element='file', **args)

@templatekeyword('files')
def showfiles(**args):
    """List of strings. All files modified, added, or removed by this
    changeset.
    """
    return showlist('file', args['ctx'].files(), **args)

@templatekeyword('graphnode')
def showgraphnode(repo, ctx, **args):
    """String. The character representing the changeset node in
    an ASCII revision graph"""
    wpnodes = repo.dirstate.parents()
    if wpnodes[1] == nullid:
        wpnodes = wpnodes[:1]
    if ctx.node() in wpnodes:
        return '@'
    elif ctx.obsolete():
        return 'x'
    elif ctx.closesbranch():
        return '_'
    else:
        return 'o'

@templatekeyword('latesttag')
def showlatesttag(**args):
    """List of strings. The global tags on the most recent globally
    tagged ancestor of this changeset.
    """
    return showlatesttags(None, **args)

def showlatesttags(pattern, **args):
    """helper method for the latesttag keyword and function"""
    repo, ctx = args['repo'], args['ctx']
    cache = args['cache']
    latesttags = getlatesttags(repo, ctx, cache, pattern)

    # latesttag[0] is an implementation detail for sorting csets on different
    # branches in a stable manner- it is the date the tagged cset was created,
    # not the date the tag was created.  Therefore it isn't made visible here.
    makemap = lambda v: {
        'changes': _showchangessincetag,
        'distance': latesttags[1],
        'latesttag': v,   # BC with {latesttag % '{latesttag}'}
        'tag': v
    }

    tags = latesttags[2]
    f = _showlist('latesttag', tags, separator=':', **args)
    return _hybrid(f, tags, makemap, lambda x: x['latesttag'])

@templatekeyword('latesttagdistance')
def showlatesttagdistance(repo, ctx, templ, cache, **args):
    """Integer. Longest path to the latest tag."""
    return getlatesttags(repo, ctx, cache)[1]

@templatekeyword('changessincelatesttag')
def showchangessincelatesttag(repo, ctx, templ, cache, **args):
    """Integer. All ancestors not in the latest tag."""
    latesttag = getlatesttags(repo, ctx, cache)[2][0]

    return _showchangessincetag(repo, ctx, tag=latesttag, **args)

def _showchangessincetag(repo, ctx, **args):
    offset = 0
    revs = [ctx.rev()]
    tag = args['tag']

    # The only() revset doesn't currently support wdir()
    if ctx.rev() is None:
        offset = 1
        revs = [p.rev() for p in ctx.parents()]

    return len(repo.revs('only(%ld, %s)', revs, tag)) + offset

@templatekeyword('manifest')
def showmanifest(**args):
    repo, ctx, templ = args['repo'], args['ctx'], args['templ']
    mnode = ctx.manifestnode()
    if mnode is None:
        # just avoid crash, we might want to use the 'ff...' hash in future
        return
    args = args.copy()
    args.update({'rev': repo.manifest.rev(mnode), 'node': hex(mnode)})
    return templ('manifest', **args)

def shownames(namespace, **args):
    """helper method to generate a template keyword for a namespace"""
    ctx = args['ctx']
    repo = ctx.repo()
    ns = repo.names[namespace]
    names = ns.names(repo, ctx.node())
    return showlist(ns.templatename, names, plural=namespace, **args)

@templatekeyword('namespaces')
def shownamespaces(**args):
    """Dict of lists. Names attached to this changeset per
    namespace."""
    ctx = args['ctx']
    repo = ctx.repo()
    namespaces = util.sortdict((k, showlist('name', ns.names(repo, ctx.node()),
                                            **args))
                               for k, ns in repo.names.iteritems())
    f = _showlist('namespace', list(namespaces), **args)
    return _hybrid(f, namespaces,
                   lambda k: {'namespace': k, 'names': namespaces[k]},
                   lambda x: x['namespace'])

@templatekeyword('node')
def shownode(repo, ctx, templ, **args):
    """String. The changeset identification hash, as a 40 hexadecimal
    digit string.
    """
    return ctx.hex()

@templatekeyword('p1rev')
def showp1rev(repo, ctx, templ, **args):
    """Integer. The repository-local revision number of the changeset's
    first parent, or -1 if the changeset has no parents."""
    return ctx.p1().rev()

@templatekeyword('p2rev')
def showp2rev(repo, ctx, templ, **args):
    """Integer. The repository-local revision number of the changeset's
    second parent, or -1 if the changeset has no second parent."""
    return ctx.p2().rev()

@templatekeyword('p1node')
def showp1node(repo, ctx, templ, **args):
    """String. The identification hash of the changeset's first parent,
    as a 40 digit hexadecimal string. If the changeset has no parents, all
    digits are 0."""
    return ctx.p1().hex()

@templatekeyword('p2node')
def showp2node(repo, ctx, templ, **args):
    """String. The identification hash of the changeset's second
    parent, as a 40 digit hexadecimal string. If the changeset has no second
    parent, all digits are 0."""
    return ctx.p2().hex()

@templatekeyword('parents')
def showparents(**args):
    """List of strings. The parents of the changeset in "rev:node"
    format. If the changeset has only one "natural" parent (the predecessor
    revision) nothing is shown."""
    repo = args['repo']
    ctx = args['ctx']
    pctxs = scmutil.meaningfulparents(repo, ctx)
    prevs = [str(p.rev()) for p in pctxs]  # ifcontains() needs a list of str
    parents = [[('rev', p.rev()),
                ('node', p.hex()),
                ('phase', p.phasestr())]
               for p in pctxs]
    f = _showlist('parent', parents, **args)
    return _hybrid(f, prevs, lambda x: {'ctx': repo[int(x)], 'revcache': {}})

@templatekeyword('phase')
def showphase(repo, ctx, templ, **args):
    """String. The changeset phase name."""
    return ctx.phasestr()

@templatekeyword('phaseidx')
def showphaseidx(repo, ctx, templ, **args):
    """Integer. The changeset phase index."""
    return ctx.phase()

@templatekeyword('rev')
def showrev(repo, ctx, templ, **args):
    """Integer. The repository-local changeset revision number."""
    return scmutil.intrev(ctx.rev())

def showrevslist(name, revs, **args):
    """helper to generate a list of revisions in which a mapped template will
    be evaluated"""
    repo = args['ctx'].repo()
    revs = [str(r) for r in revs]  # ifcontains() needs a list of str
    f = _showlist(name, revs, **args)
    return _hybrid(f, revs,
                   lambda x: {name: x, 'ctx': repo[int(x)], 'revcache': {}})

@templatekeyword('subrepos')
def showsubrepos(**args):
    """List of strings. Updated subrepositories in the changeset."""
    ctx = args['ctx']
    substate = ctx.substate
    if not substate:
        return showlist('subrepo', [], **args)
    psubstate = ctx.parents()[0].substate or {}
    subrepos = []
    for sub in substate:
        if sub not in psubstate or substate[sub] != psubstate[sub]:
            subrepos.append(sub) # modified or newly added in ctx
    for sub in psubstate:
        if sub not in substate:
            subrepos.append(sub) # removed in ctx
    return showlist('subrepo', sorted(subrepos), **args)

# don't remove "showtags" definition, even though namespaces will put
# a helper function for "tags" keyword into "keywords" map automatically,
# because online help text is built without namespaces initialization
@templatekeyword('tags')
def showtags(**args):
    """List of strings. Any tags associated with the changeset."""
    return shownames('tags', **args)

def loadkeyword(ui, extname, registrarobj):
    """Load template keyword from specified registrarobj
    """
    for name, func in registrarobj._table.iteritems():
        keywords[name] = func

# tell hggettext to extract docstrings from these functions:
i18nfunctions = keywords.values()
