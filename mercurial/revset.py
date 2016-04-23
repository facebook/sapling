# revset.py - revision set queries for mercurial
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import heapq
import re

from .i18n import _
from . import (
    destutil,
    encoding,
    error,
    hbisect,
    match as matchmod,
    node,
    obsolete as obsmod,
    parser,
    pathutil,
    phases,
    registrar,
    repoview,
    util,
)

def _revancestors(repo, revs, followfirst):
    """Like revlog.ancestors(), but supports followfirst."""
    if followfirst:
        cut = 1
    else:
        cut = None
    cl = repo.changelog

    def iterate():
        revs.sort(reverse=True)
        irevs = iter(revs)
        h = []

        inputrev = next(irevs, None)
        if inputrev is not None:
            heapq.heappush(h, -inputrev)

        seen = set()
        while h:
            current = -heapq.heappop(h)
            if current == inputrev:
                inputrev = next(irevs, None)
                if inputrev is not None:
                    heapq.heappush(h, -inputrev)
            if current not in seen:
                seen.add(current)
                yield current
                for parent in cl.parentrevs(current)[:cut]:
                    if parent != node.nullrev:
                        heapq.heappush(h, -parent)

    return generatorset(iterate(), iterasc=False)

def _revdescendants(repo, revs, followfirst):
    """Like revlog.descendants() but supports followfirst."""
    if followfirst:
        cut = 1
    else:
        cut = None

    def iterate():
        cl = repo.changelog
        # XXX this should be 'parentset.min()' assuming 'parentset' is a
        # smartset (and if it is not, it should.)
        first = min(revs)
        nullrev = node.nullrev
        if first == nullrev:
            # Are there nodes with a null first parent and a non-null
            # second one? Maybe. Do we care? Probably not.
            for i in cl:
                yield i
        else:
            seen = set(revs)
            for i in cl.revs(first + 1):
                for x in cl.parentrevs(i)[:cut]:
                    if x != nullrev and x in seen:
                        seen.add(i)
                        yield i
                        break

    return generatorset(iterate(), iterasc=True)

def _reachablerootspure(repo, minroot, roots, heads, includepath):
    """return (heads(::<roots> and ::<heads>))

    If includepath is True, return (<roots>::<heads>)."""
    if not roots:
        return []
    parentrevs = repo.changelog.parentrevs
    roots = set(roots)
    visit = list(heads)
    reachable = set()
    seen = {}
    # prefetch all the things! (because python is slow)
    reached = reachable.add
    dovisit = visit.append
    nextvisit = visit.pop
    # open-code the post-order traversal due to the tiny size of
    # sys.getrecursionlimit()
    while visit:
        rev = nextvisit()
        if rev in roots:
            reached(rev)
            if not includepath:
                continue
        parents = parentrevs(rev)
        seen[rev] = parents
        for parent in parents:
            if parent >= minroot and parent not in seen:
                dovisit(parent)
    if not reachable:
        return baseset()
    if not includepath:
        return reachable
    for rev in sorted(seen):
        for parent in seen[rev]:
            if parent in reachable:
                reached(rev)
    return reachable

def reachableroots(repo, roots, heads, includepath=False):
    """return (heads(::<roots> and ::<heads>))

    If includepath is True, return (<roots>::<heads>)."""
    if not roots:
        return baseset()
    minroot = roots.min()
    roots = list(roots)
    heads = list(heads)
    try:
        revs = repo.changelog.reachableroots(minroot, heads, roots, includepath)
    except AttributeError:
        revs = _reachablerootspure(repo, minroot, roots, heads, includepath)
    revs = baseset(revs)
    revs.sort()
    return revs

elements = {
    # token-type: binding-strength, primary, prefix, infix, suffix
    "(": (21, None, ("group", 1, ")"), ("func", 1, ")"), None),
    "##": (20, None, None, ("_concat", 20), None),
    "~": (18, None, None, ("ancestor", 18), None),
    "^": (18, None, None, ("parent", 18), ("parentpost", 18)),
    "-": (5, None, ("negate", 19), ("minus", 5), None),
    "::": (17, None, ("dagrangepre", 17), ("dagrange", 17),
           ("dagrangepost", 17)),
    "..": (17, None, ("dagrangepre", 17), ("dagrange", 17),
           ("dagrangepost", 17)),
    ":": (15, "rangeall", ("rangepre", 15), ("range", 15), ("rangepost", 15)),
    "not": (10, None, ("not", 10), None, None),
    "!": (10, None, ("not", 10), None, None),
    "and": (5, None, None, ("and", 5), None),
    "&": (5, None, None, ("and", 5), None),
    "%": (5, None, None, ("only", 5), ("onlypost", 5)),
    "or": (4, None, None, ("or", 4), None),
    "|": (4, None, None, ("or", 4), None),
    "+": (4, None, None, ("or", 4), None),
    "=": (3, None, None, ("keyvalue", 3), None),
    ",": (2, None, None, ("list", 2), None),
    ")": (0, None, None, None, None),
    "symbol": (0, "symbol", None, None, None),
    "string": (0, "string", None, None, None),
    "end": (0, None, None, None, None),
}

keywords = set(['and', 'or', 'not'])

# default set of valid characters for the initial letter of symbols
_syminitletters = set(c for c in [chr(i) for i in xrange(256)]
                      if c.isalnum() or c in '._@' or ord(c) > 127)

# default set of valid characters for non-initial letters of symbols
_symletters = set(c for c in  [chr(i) for i in xrange(256)]
                  if c.isalnum() or c in '-._/@' or ord(c) > 127)

def tokenize(program, lookup=None, syminitletters=None, symletters=None):
    '''
    Parse a revset statement into a stream of tokens

    ``syminitletters`` is the set of valid characters for the initial
    letter of symbols.

    By default, character ``c`` is recognized as valid for initial
    letter of symbols, if ``c.isalnum() or c in '._@' or ord(c) > 127``.

    ``symletters`` is the set of valid characters for non-initial
    letters of symbols.

    By default, character ``c`` is recognized as valid for non-initial
    letters of symbols, if ``c.isalnum() or c in '-._/@' or ord(c) > 127``.

    Check that @ is a valid unquoted token character (issue3686):
    >>> list(tokenize("@::"))
    [('symbol', '@', 0), ('::', None, 1), ('end', None, 3)]

    '''
    if syminitletters is None:
        syminitletters = _syminitletters
    if symletters is None:
        symletters = _symletters

    if program and lookup:
        # attempt to parse old-style ranges first to deal with
        # things like old-tag which contain query metacharacters
        parts = program.split(':', 1)
        if all(lookup(sym) for sym in parts if sym):
            if parts[0]:
                yield ('symbol', parts[0], 0)
            if len(parts) > 1:
                s = len(parts[0])
                yield (':', None, s)
                if parts[1]:
                    yield ('symbol', parts[1], s + 1)
            yield ('end', None, len(program))
            return

    pos, l = 0, len(program)
    while pos < l:
        c = program[pos]
        if c.isspace(): # skip inter-token whitespace
            pass
        elif c == ':' and program[pos:pos + 2] == '::': # look ahead carefully
            yield ('::', None, pos)
            pos += 1 # skip ahead
        elif c == '.' and program[pos:pos + 2] == '..': # look ahead carefully
            yield ('..', None, pos)
            pos += 1 # skip ahead
        elif c == '#' and program[pos:pos + 2] == '##': # look ahead carefully
            yield ('##', None, pos)
            pos += 1 # skip ahead
        elif c in "():=,-|&+!~^%": # handle simple operators
            yield (c, None, pos)
        elif (c in '"\'' or c == 'r' and
              program[pos:pos + 2] in ("r'", 'r"')): # handle quoted strings
            if c == 'r':
                pos += 1
                c = program[pos]
                decode = lambda x: x
            else:
                decode = parser.unescapestr
            pos += 1
            s = pos
            while pos < l: # find closing quote
                d = program[pos]
                if d == '\\': # skip over escaped characters
                    pos += 2
                    continue
                if d == c:
                    yield ('string', decode(program[s:pos]), s)
                    break
                pos += 1
            else:
                raise error.ParseError(_("unterminated string"), s)
        # gather up a symbol/keyword
        elif c in syminitletters:
            s = pos
            pos += 1
            while pos < l: # find end of symbol
                d = program[pos]
                if d not in symletters:
                    break
                if d == '.' and program[pos - 1] == '.': # special case for ..
                    pos -= 1
                    break
                pos += 1
            sym = program[s:pos]
            if sym in keywords: # operator keywords
                yield (sym, None, s)
            elif '-' in sym:
                # some jerk gave us foo-bar-baz, try to check if it's a symbol
                if lookup and lookup(sym):
                    # looks like a real symbol
                    yield ('symbol', sym, s)
                else:
                    # looks like an expression
                    parts = sym.split('-')
                    for p in parts[:-1]:
                        if p: # possible consecutive -
                            yield ('symbol', p, s)
                        s += len(p)
                        yield ('-', None, pos)
                        s += 1
                    if parts[-1]: # possible trailing -
                        yield ('symbol', parts[-1], s)
            else:
                yield ('symbol', sym, s)
            pos -= 1
        else:
            raise error.ParseError(_("syntax error in revset '%s'") %
                                   program, pos)
        pos += 1
    yield ('end', None, pos)

# helpers

def getstring(x, err):
    if x and (x[0] == 'string' or x[0] == 'symbol'):
        return x[1]
    raise error.ParseError(err)

def getlist(x):
    if not x:
        return []
    if x[0] == 'list':
        return list(x[1:])
    return [x]

def getargs(x, min, max, err):
    l = getlist(x)
    if len(l) < min or (max >= 0 and len(l) > max):
        raise error.ParseError(err)
    return l

def getargsdict(x, funcname, keys):
    return parser.buildargsdict(getlist(x), funcname, keys.split(),
                                keyvaluenode='keyvalue', keynode='symbol')

def getset(repo, subset, x):
    if not x:
        raise error.ParseError(_("missing argument"))
    s = methods[x[0]](repo, subset, *x[1:])
    if util.safehasattr(s, 'isascending'):
        return s
    if (repo.ui.configbool('devel', 'all-warnings')
            or repo.ui.configbool('devel', 'old-revset')):
        # else case should not happen, because all non-func are internal,
        # ignoring for now.
        if x[0] == 'func' and x[1][0] == 'symbol' and x[1][1] in symbols:
            repo.ui.develwarn('revset "%s" use list instead of smartset, '
                              '(upgrade your code)' % x[1][1])
    return baseset(s)

def _getrevsource(repo, r):
    extra = repo[r].extra()
    for label in ('source', 'transplant_source', 'rebase_source'):
        if label in extra:
            try:
                return repo[extra[label]].rev()
            except error.RepoLookupError:
                pass
    return None

# operator methods

def stringset(repo, subset, x):
    x = repo[x].rev()
    if (x in subset
        or x == node.nullrev and isinstance(subset, fullreposet)):
        return baseset([x])
    return baseset()

def rangeset(repo, subset, x, y):
    m = getset(repo, fullreposet(repo), x)
    n = getset(repo, fullreposet(repo), y)

    if not m or not n:
        return baseset()
    m, n = m.first(), n.last()

    if m == n:
        r = baseset([m])
    elif n == node.wdirrev:
        r = spanset(repo, m, len(repo)) + baseset([n])
    elif m == node.wdirrev:
        r = baseset([m]) + spanset(repo, len(repo) - 1, n - 1)
    elif m < n:
        r = spanset(repo, m, n + 1)
    else:
        r = spanset(repo, m, n - 1)
    # XXX We should combine with subset first: 'subset & baseset(...)'. This is
    # necessary to ensure we preserve the order in subset.
    #
    # This has performance implication, carrying the sorting over when possible
    # would be more efficient.
    return r & subset

def dagrange(repo, subset, x, y):
    r = fullreposet(repo)
    xs = reachableroots(repo, getset(repo, r, x), getset(repo, r, y),
                         includepath=True)
    # XXX We should combine with subset first: 'subset & baseset(...)'. This is
    # necessary to ensure we preserve the order in subset.
    return xs & subset

def andset(repo, subset, x, y):
    return getset(repo, getset(repo, subset, x), y)

def differenceset(repo, subset, x, y):
    return getset(repo, subset, x) - getset(repo, subset, y)

def orset(repo, subset, *xs):
    assert xs
    if len(xs) == 1:
        return getset(repo, subset, xs[0])
    p = len(xs) // 2
    a = orset(repo, subset, *xs[:p])
    b = orset(repo, subset, *xs[p:])
    return a + b

def notset(repo, subset, x):
    return subset - getset(repo, subset, x)

def listset(repo, subset, *xs):
    raise error.ParseError(_("can't use a list in this context"),
                           hint=_('see hg help "revsets.x or y"'))

def keyvaluepair(repo, subset, k, v):
    raise error.ParseError(_("can't use a key-value pair in this context"))

def func(repo, subset, a, b):
    if a[0] == 'symbol' and a[1] in symbols:
        return symbols[a[1]](repo, subset, b)

    keep = lambda fn: getattr(fn, '__doc__', None) is not None

    syms = [s for (s, fn) in symbols.items() if keep(fn)]
    raise error.UnknownIdentifier(a[1], syms)

# functions

# symbols are callables like:
#   fn(repo, subset, x)
# with:
#   repo - current repository instance
#   subset - of revisions to be examined
#   x - argument in tree form
symbols = {}

# symbols which can't be used for a DoS attack for any given input
# (e.g. those which accept regexes as plain strings shouldn't be included)
# functions that just return a lot of changesets (like all) don't count here
safesymbols = set()

predicate = registrar.revsetpredicate()

@predicate('_destupdate')
def _destupdate(repo, subset, x):
    # experimental revset for update destination
    args = getargsdict(x, 'limit', 'clean check')
    return subset & baseset([destutil.destupdate(repo, **args)[0]])

@predicate('_destmerge')
def _destmerge(repo, subset, x):
    # experimental revset for merge destination
    sourceset = None
    if x is not None:
        sourceset = getset(repo, fullreposet(repo), x)
    return subset & baseset([destutil.destmerge(repo, sourceset=sourceset)])

@predicate('adds(pattern)', safe=True)
def adds(repo, subset, x):
    """Changesets that add a file matching pattern.

    The pattern without explicit kind like ``glob:`` is expected to be
    relative to the current directory and match against a file or a
    directory.
    """
    # i18n: "adds" is a keyword
    pat = getstring(x, _("adds requires a pattern"))
    return checkstatus(repo, subset, pat, 1)

@predicate('ancestor(*changeset)', safe=True)
def ancestor(repo, subset, x):
    """A greatest common ancestor of the changesets.

    Accepts 0 or more changesets.
    Will return empty list when passed no args.
    Greatest common ancestor of a single changeset is that changeset.
    """
    # i18n: "ancestor" is a keyword
    l = getlist(x)
    rl = fullreposet(repo)
    anc = None

    # (getset(repo, rl, i) for i in l) generates a list of lists
    for revs in (getset(repo, rl, i) for i in l):
        for r in revs:
            if anc is None:
                anc = repo[r]
            else:
                anc = anc.ancestor(repo[r])

    if anc is not None and anc.rev() in subset:
        return baseset([anc.rev()])
    return baseset()

def _ancestors(repo, subset, x, followfirst=False):
    heads = getset(repo, fullreposet(repo), x)
    if not heads:
        return baseset()
    s = _revancestors(repo, heads, followfirst)
    return subset & s

@predicate('ancestors(set)', safe=True)
def ancestors(repo, subset, x):
    """Changesets that are ancestors of a changeset in set.
    """
    return _ancestors(repo, subset, x)

@predicate('_firstancestors', safe=True)
def _firstancestors(repo, subset, x):
    # ``_firstancestors(set)``
    # Like ``ancestors(set)`` but follows only the first parents.
    return _ancestors(repo, subset, x, followfirst=True)

def ancestorspec(repo, subset, x, n):
    """``set~n``
    Changesets that are the Nth ancestor (first parents only) of a changeset
    in set.
    """
    try:
        n = int(n[1])
    except (TypeError, ValueError):
        raise error.ParseError(_("~ expects a number"))
    ps = set()
    cl = repo.changelog
    for r in getset(repo, fullreposet(repo), x):
        for i in range(n):
            r = cl.parentrevs(r)[0]
        ps.add(r)
    return subset & ps

@predicate('author(string)', safe=True)
def author(repo, subset, x):
    """Alias for ``user(string)``.
    """
    # i18n: "author" is a keyword
    n = encoding.lower(getstring(x, _("author requires a string")))
    kind, pattern, matcher = _substringmatcher(n)
    return subset.filter(lambda x: matcher(encoding.lower(repo[x].user())),
                         condrepr=('<user %r>', n))

@predicate('bisect(string)', safe=True)
def bisect(repo, subset, x):
    """Changesets marked in the specified bisect status:

    - ``good``, ``bad``, ``skip``: csets explicitly marked as good/bad/skip
    - ``goods``, ``bads``      : csets topologically good/bad
    - ``range``              : csets taking part in the bisection
    - ``pruned``             : csets that are goods, bads or skipped
    - ``untested``           : csets whose fate is yet unknown
    - ``ignored``            : csets ignored due to DAG topology
    - ``current``            : the cset currently being bisected
    """
    # i18n: "bisect" is a keyword
    status = getstring(x, _("bisect requires a string")).lower()
    state = set(hbisect.get(repo, status))
    return subset & state

# Backward-compatibility
# - no help entry so that we do not advertise it any more
@predicate('bisected', safe=True)
def bisected(repo, subset, x):
    return bisect(repo, subset, x)

@predicate('bookmark([name])', safe=True)
def bookmark(repo, subset, x):
    """The named bookmark or all bookmarks.

    If `name` starts with `re:`, the remainder of the name is treated as
    a regular expression. To match a bookmark that actually starts with `re:`,
    use the prefix `literal:`.
    """
    # i18n: "bookmark" is a keyword
    args = getargs(x, 0, 1, _('bookmark takes one or no arguments'))
    if args:
        bm = getstring(args[0],
                       # i18n: "bookmark" is a keyword
                       _('the argument to bookmark must be a string'))
        kind, pattern, matcher = util.stringmatcher(bm)
        bms = set()
        if kind == 'literal':
            bmrev = repo._bookmarks.get(pattern, None)
            if not bmrev:
                raise error.RepoLookupError(_("bookmark '%s' does not exist")
                                            % pattern)
            bms.add(repo[bmrev].rev())
        else:
            matchrevs = set()
            for name, bmrev in repo._bookmarks.iteritems():
                if matcher(name):
                    matchrevs.add(bmrev)
            if not matchrevs:
                raise error.RepoLookupError(_("no bookmarks exist"
                                              " that match '%s'") % pattern)
            for bmrev in matchrevs:
                bms.add(repo[bmrev].rev())
    else:
        bms = set([repo[r].rev()
                   for r in repo._bookmarks.values()])
    bms -= set([node.nullrev])
    return subset & bms

@predicate('branch(string or set)', safe=True)
def branch(repo, subset, x):
    """
    All changesets belonging to the given branch or the branches of the given
    changesets.

    If `string` starts with `re:`, the remainder of the name is treated as
    a regular expression. To match a branch that actually starts with `re:`,
    use the prefix `literal:`.
    """
    getbi = repo.revbranchcache().branchinfo

    try:
        b = getstring(x, '')
    except error.ParseError:
        # not a string, but another revspec, e.g. tip()
        pass
    else:
        kind, pattern, matcher = util.stringmatcher(b)
        if kind == 'literal':
            # note: falls through to the revspec case if no branch with
            # this name exists and pattern kind is not specified explicitly
            if pattern in repo.branchmap():
                return subset.filter(lambda r: matcher(getbi(r)[0]),
                                     condrepr=('<branch %r>', b))
            if b.startswith('literal:'):
                raise error.RepoLookupError(_("branch '%s' does not exist")
                                            % pattern)
        else:
            return subset.filter(lambda r: matcher(getbi(r)[0]),
                                 condrepr=('<branch %r>', b))

    s = getset(repo, fullreposet(repo), x)
    b = set()
    for r in s:
        b.add(getbi(r)[0])
    c = s.__contains__
    return subset.filter(lambda r: c(r) or getbi(r)[0] in b,
                         condrepr=lambda: '<branch %r>' % sorted(b))

@predicate('bumped()', safe=True)
def bumped(repo, subset, x):
    """Mutable changesets marked as successors of public changesets.

    Only non-public and non-obsolete changesets can be `bumped`.
    """
    # i18n: "bumped" is a keyword
    getargs(x, 0, 0, _("bumped takes no arguments"))
    bumped = obsmod.getrevs(repo, 'bumped')
    return subset & bumped

@predicate('bundle()', safe=True)
def bundle(repo, subset, x):
    """Changesets in the bundle.

    Bundle must be specified by the -R option."""

    try:
        bundlerevs = repo.changelog.bundlerevs
    except AttributeError:
        raise error.Abort(_("no bundle provided - specify with -R"))
    return subset & bundlerevs

def checkstatus(repo, subset, pat, field):
    hasset = matchmod.patkind(pat) == 'set'

    mcache = [None]
    def matches(x):
        c = repo[x]
        if not mcache[0] or hasset:
            mcache[0] = matchmod.match(repo.root, repo.getcwd(), [pat], ctx=c)
        m = mcache[0]
        fname = None
        if not m.anypats() and len(m.files()) == 1:
            fname = m.files()[0]
        if fname is not None:
            if fname not in c.files():
                return False
        else:
            for f in c.files():
                if m(f):
                    break
            else:
                return False
        files = repo.status(c.p1().node(), c.node())[field]
        if fname is not None:
            if fname in files:
                return True
        else:
            for f in files:
                if m(f):
                    return True

    return subset.filter(matches, condrepr=('<status[%r] %r>', field, pat))

def _children(repo, narrow, parentset):
    if not parentset:
        return baseset()
    cs = set()
    pr = repo.changelog.parentrevs
    minrev = parentset.min()
    for r in narrow:
        if r <= minrev:
            continue
        for p in pr(r):
            if p in parentset:
                cs.add(r)
    # XXX using a set to feed the baseset is wrong. Sets are not ordered.
    # This does not break because of other fullreposet misbehavior.
    return baseset(cs)

@predicate('children(set)', safe=True)
def children(repo, subset, x):
    """Child changesets of changesets in set.
    """
    s = getset(repo, fullreposet(repo), x)
    cs = _children(repo, subset, s)
    return subset & cs

@predicate('closed()', safe=True)
def closed(repo, subset, x):
    """Changeset is closed.
    """
    # i18n: "closed" is a keyword
    getargs(x, 0, 0, _("closed takes no arguments"))
    return subset.filter(lambda r: repo[r].closesbranch(),
                         condrepr='<branch closed>')

@predicate('contains(pattern)')
def contains(repo, subset, x):
    """The revision's manifest contains a file matching pattern (but might not
    modify it). See :hg:`help patterns` for information about file patterns.

    The pattern without explicit kind like ``glob:`` is expected to be
    relative to the current directory and match against a file exactly
    for efficiency.
    """
    # i18n: "contains" is a keyword
    pat = getstring(x, _("contains requires a pattern"))

    def matches(x):
        if not matchmod.patkind(pat):
            pats = pathutil.canonpath(repo.root, repo.getcwd(), pat)
            if pats in repo[x]:
                return True
        else:
            c = repo[x]
            m = matchmod.match(repo.root, repo.getcwd(), [pat], ctx=c)
            for f in c.manifest():
                if m(f):
                    return True
        return False

    return subset.filter(matches, condrepr=('<contains %r>', pat))

@predicate('converted([id])', safe=True)
def converted(repo, subset, x):
    """Changesets converted from the given identifier in the old repository if
    present, or all converted changesets if no identifier is specified.
    """

    # There is exactly no chance of resolving the revision, so do a simple
    # string compare and hope for the best

    rev = None
    # i18n: "converted" is a keyword
    l = getargs(x, 0, 1, _('converted takes one or no arguments'))
    if l:
        # i18n: "converted" is a keyword
        rev = getstring(l[0], _('converted requires a revision'))

    def _matchvalue(r):
        source = repo[r].extra().get('convert_revision', None)
        return source is not None and (rev is None or source.startswith(rev))

    return subset.filter(lambda r: _matchvalue(r),
                         condrepr=('<converted %r>', rev))

@predicate('date(interval)', safe=True)
def date(repo, subset, x):
    """Changesets within the interval, see :hg:`help dates`.
    """
    # i18n: "date" is a keyword
    ds = getstring(x, _("date requires a string"))
    dm = util.matchdate(ds)
    return subset.filter(lambda x: dm(repo[x].date()[0]),
                         condrepr=('<date %r>', ds))

@predicate('desc(string)', safe=True)
def desc(repo, subset, x):
    """Search commit message for string. The match is case-insensitive.
    """
    # i18n: "desc" is a keyword
    ds = encoding.lower(getstring(x, _("desc requires a string")))

    def matches(x):
        c = repo[x]
        return ds in encoding.lower(c.description())

    return subset.filter(matches, condrepr=('<desc %r>', ds))

def _descendants(repo, subset, x, followfirst=False):
    roots = getset(repo, fullreposet(repo), x)
    if not roots:
        return baseset()
    s = _revdescendants(repo, roots, followfirst)

    # Both sets need to be ascending in order to lazily return the union
    # in the correct order.
    base = subset & roots
    desc = subset & s
    result = base + desc
    if subset.isascending():
        result.sort()
    elif subset.isdescending():
        result.sort(reverse=True)
    else:
        result = subset & result
    return result

@predicate('descendants(set)', safe=True)
def descendants(repo, subset, x):
    """Changesets which are descendants of changesets in set.
    """
    return _descendants(repo, subset, x)

@predicate('_firstdescendants', safe=True)
def _firstdescendants(repo, subset, x):
    # ``_firstdescendants(set)``
    # Like ``descendants(set)`` but follows only the first parents.
    return _descendants(repo, subset, x, followfirst=True)

@predicate('destination([set])', safe=True)
def destination(repo, subset, x):
    """Changesets that were created by a graft, transplant or rebase operation,
    with the given revisions specified as the source.  Omitting the optional set
    is the same as passing all().
    """
    if x is not None:
        sources = getset(repo, fullreposet(repo), x)
    else:
        sources = fullreposet(repo)

    dests = set()

    # subset contains all of the possible destinations that can be returned, so
    # iterate over them and see if their source(s) were provided in the arg set.
    # Even if the immediate src of r is not in the arg set, src's source (or
    # further back) may be.  Scanning back further than the immediate src allows
    # transitive transplants and rebases to yield the same results as transitive
    # grafts.
    for r in subset:
        src = _getrevsource(repo, r)
        lineage = None

        while src is not None:
            if lineage is None:
                lineage = list()

            lineage.append(r)

            # The visited lineage is a match if the current source is in the arg
            # set.  Since every candidate dest is visited by way of iterating
            # subset, any dests further back in the lineage will be tested by a
            # different iteration over subset.  Likewise, if the src was already
            # selected, the current lineage can be selected without going back
            # further.
            if src in sources or src in dests:
                dests.update(lineage)
                break

            r = src
            src = _getrevsource(repo, r)

    return subset.filter(dests.__contains__,
                         condrepr=lambda: '<destination %r>' % sorted(dests))

@predicate('divergent()', safe=True)
def divergent(repo, subset, x):
    """
    Final successors of changesets with an alternative set of final successors.
    """
    # i18n: "divergent" is a keyword
    getargs(x, 0, 0, _("divergent takes no arguments"))
    divergent = obsmod.getrevs(repo, 'divergent')
    return subset & divergent

@predicate('extinct()', safe=True)
def extinct(repo, subset, x):
    """Obsolete changesets with obsolete descendants only.
    """
    # i18n: "extinct" is a keyword
    getargs(x, 0, 0, _("extinct takes no arguments"))
    extincts = obsmod.getrevs(repo, 'extinct')
    return subset & extincts

@predicate('extra(label, [value])', safe=True)
def extra(repo, subset, x):
    """Changesets with the given label in the extra metadata, with the given
    optional value.

    If `value` starts with `re:`, the remainder of the value is treated as
    a regular expression. To match a value that actually starts with `re:`,
    use the prefix `literal:`.
    """
    args = getargsdict(x, 'extra', 'label value')
    if 'label' not in args:
        # i18n: "extra" is a keyword
        raise error.ParseError(_('extra takes at least 1 argument'))
    # i18n: "extra" is a keyword
    label = getstring(args['label'], _('first argument to extra must be '
                                       'a string'))
    value = None

    if 'value' in args:
        # i18n: "extra" is a keyword
        value = getstring(args['value'], _('second argument to extra must be '
                                           'a string'))
        kind, value, matcher = util.stringmatcher(value)

    def _matchvalue(r):
        extra = repo[r].extra()
        return label in extra and (value is None or matcher(extra[label]))

    return subset.filter(lambda r: _matchvalue(r),
                         condrepr=('<extra[%r] %r>', label, value))

@predicate('filelog(pattern)', safe=True)
def filelog(repo, subset, x):
    """Changesets connected to the specified filelog.

    For performance reasons, visits only revisions mentioned in the file-level
    filelog, rather than filtering through all changesets (much faster, but
    doesn't include deletes or duplicate changes). For a slower, more accurate
    result, use ``file()``.

    The pattern without explicit kind like ``glob:`` is expected to be
    relative to the current directory and match against a file exactly
    for efficiency.

    If some linkrev points to revisions filtered by the current repoview, we'll
    work around it to return a non-filtered value.
    """

    # i18n: "filelog" is a keyword
    pat = getstring(x, _("filelog requires a pattern"))
    s = set()
    cl = repo.changelog

    if not matchmod.patkind(pat):
        f = pathutil.canonpath(repo.root, repo.getcwd(), pat)
        files = [f]
    else:
        m = matchmod.match(repo.root, repo.getcwd(), [pat], ctx=repo[None])
        files = (f for f in repo[None] if m(f))

    for f in files:
        fl = repo.file(f)
        known = {}
        scanpos = 0
        for fr in list(fl):
            fn = fl.node(fr)
            if fn in known:
                s.add(known[fn])
                continue

            lr = fl.linkrev(fr)
            if lr in cl:
                s.add(lr)
            elif scanpos is not None:
                # lowest matching changeset is filtered, scan further
                # ahead in changelog
                start = max(lr, scanpos) + 1
                scanpos = None
                for r in cl.revs(start):
                    # minimize parsing of non-matching entries
                    if f in cl.revision(r) and f in cl.readfiles(r):
                        try:
                            # try to use manifest delta fastpath
                            n = repo[r].filenode(f)
                            if n not in known:
                                if n == fn:
                                    s.add(r)
                                    scanpos = r
                                    break
                                else:
                                    known[n] = r
                        except error.ManifestLookupError:
                            # deletion in changelog
                            continue

    return subset & s

@predicate('first(set, [n])', safe=True)
def first(repo, subset, x):
    """An alias for limit().
    """
    return limit(repo, subset, x)

def _follow(repo, subset, x, name, followfirst=False):
    l = getargs(x, 0, 1, _("%s takes no arguments or a pattern") % name)
    c = repo['.']
    if l:
        x = getstring(l[0], _("%s expected a pattern") % name)
        matcher = matchmod.match(repo.root, repo.getcwd(), [x],
                                 ctx=repo[None], default='path')

        files = c.manifest().walk(matcher)

        s = set()
        for fname in files:
            fctx = c[fname]
            s = s.union(set(c.rev() for c in fctx.ancestors(followfirst)))
            # include the revision responsible for the most recent version
            s.add(fctx.introrev())
    else:
        s = _revancestors(repo, baseset([c.rev()]), followfirst)

    return subset & s

@predicate('follow([pattern])', safe=True)
def follow(repo, subset, x):
    """
    An alias for ``::.`` (ancestors of the working directory's first parent).
    If pattern is specified, the histories of files matching given
    pattern is followed, including copies.
    """
    return _follow(repo, subset, x, 'follow')

@predicate('_followfirst', safe=True)
def _followfirst(repo, subset, x):
    # ``followfirst([pattern])``
    # Like ``follow([pattern])`` but follows only the first parent of
    # every revisions or files revisions.
    return _follow(repo, subset, x, '_followfirst', followfirst=True)

@predicate('all()', safe=True)
def getall(repo, subset, x):
    """All changesets, the same as ``0:tip``.
    """
    # i18n: "all" is a keyword
    getargs(x, 0, 0, _("all takes no arguments"))
    return subset & spanset(repo)  # drop "null" if any

@predicate('grep(regex)')
def grep(repo, subset, x):
    """Like ``keyword(string)`` but accepts a regex. Use ``grep(r'...')``
    to ensure special escape characters are handled correctly. Unlike
    ``keyword(string)``, the match is case-sensitive.
    """
    try:
        # i18n: "grep" is a keyword
        gr = re.compile(getstring(x, _("grep requires a string")))
    except re.error as e:
        raise error.ParseError(_('invalid match pattern: %s') % e)

    def matches(x):
        c = repo[x]
        for e in c.files() + [c.user(), c.description()]:
            if gr.search(e):
                return True
        return False

    return subset.filter(matches, condrepr=('<grep %r>', gr.pattern))

@predicate('_matchfiles', safe=True)
def _matchfiles(repo, subset, x):
    # _matchfiles takes a revset list of prefixed arguments:
    #
    #   [p:foo, i:bar, x:baz]
    #
    # builds a match object from them and filters subset. Allowed
    # prefixes are 'p:' for regular patterns, 'i:' for include
    # patterns and 'x:' for exclude patterns. Use 'r:' prefix to pass
    # a revision identifier, or the empty string to reference the
    # working directory, from which the match object is
    # initialized. Use 'd:' to set the default matching mode, default
    # to 'glob'. At most one 'r:' and 'd:' argument can be passed.

    l = getargs(x, 1, -1, "_matchfiles requires at least one argument")
    pats, inc, exc = [], [], []
    rev, default = None, None
    for arg in l:
        s = getstring(arg, "_matchfiles requires string arguments")
        prefix, value = s[:2], s[2:]
        if prefix == 'p:':
            pats.append(value)
        elif prefix == 'i:':
            inc.append(value)
        elif prefix == 'x:':
            exc.append(value)
        elif prefix == 'r:':
            if rev is not None:
                raise error.ParseError('_matchfiles expected at most one '
                                       'revision')
            if value != '': # empty means working directory; leave rev as None
                rev = value
        elif prefix == 'd:':
            if default is not None:
                raise error.ParseError('_matchfiles expected at most one '
                                       'default mode')
            default = value
        else:
            raise error.ParseError('invalid _matchfiles prefix: %s' % prefix)
    if not default:
        default = 'glob'

    m = matchmod.match(repo.root, repo.getcwd(), pats, include=inc,
                       exclude=exc, ctx=repo[rev], default=default)

    # This directly read the changelog data as creating changectx for all
    # revisions is quite expensive.
    getfiles = repo.changelog.readfiles
    wdirrev = node.wdirrev
    def matches(x):
        if x == wdirrev:
            files = repo[x].files()
        else:
            files = getfiles(x)
        for f in files:
            if m(f):
                return True
        return False

    return subset.filter(matches,
                         condrepr=('<matchfiles patterns=%r, include=%r '
                                   'exclude=%r, default=%r, rev=%r>',
                                   pats, inc, exc, default, rev))

@predicate('file(pattern)', safe=True)
def hasfile(repo, subset, x):
    """Changesets affecting files matched by pattern.

    For a faster but less accurate result, consider using ``filelog()``
    instead.

    This predicate uses ``glob:`` as the default kind of pattern.
    """
    # i18n: "file" is a keyword
    pat = getstring(x, _("file requires a pattern"))
    return _matchfiles(repo, subset, ('string', 'p:' + pat))

@predicate('head()', safe=True)
def head(repo, subset, x):
    """Changeset is a named branch head.
    """
    # i18n: "head" is a keyword
    getargs(x, 0, 0, _("head takes no arguments"))
    hs = set()
    cl = repo.changelog
    for b, ls in repo.branchmap().iteritems():
        hs.update(cl.rev(h) for h in ls)
    # XXX using a set to feed the baseset is wrong. Sets are not ordered.
    # This does not break because of other fullreposet misbehavior.
    # XXX We should combine with subset first: 'subset & baseset(...)'. This is
    # necessary to ensure we preserve the order in subset.
    return baseset(hs) & subset

@predicate('heads(set)', safe=True)
def heads(repo, subset, x):
    """Members of set with no children in set.
    """
    s = getset(repo, subset, x)
    ps = parents(repo, subset, x)
    return s - ps

@predicate('hidden()', safe=True)
def hidden(repo, subset, x):
    """Hidden changesets.
    """
    # i18n: "hidden" is a keyword
    getargs(x, 0, 0, _("hidden takes no arguments"))
    hiddenrevs = repoview.filterrevs(repo, 'visible')
    return subset & hiddenrevs

@predicate('keyword(string)', safe=True)
def keyword(repo, subset, x):
    """Search commit message, user name, and names of changed files for
    string. The match is case-insensitive.
    """
    # i18n: "keyword" is a keyword
    kw = encoding.lower(getstring(x, _("keyword requires a string")))

    def matches(r):
        c = repo[r]
        return any(kw in encoding.lower(t)
                   for t in c.files() + [c.user(), c.description()])

    return subset.filter(matches, condrepr=('<keyword %r>', kw))

@predicate('limit(set[, n[, offset]])', safe=True)
def limit(repo, subset, x):
    """First n members of set, defaulting to 1, starting from offset.
    """
    args = getargsdict(x, 'limit', 'set n offset')
    if 'set' not in args:
        # i18n: "limit" is a keyword
        raise error.ParseError(_("limit requires one to three arguments"))
    try:
        lim, ofs = 1, 0
        if 'n' in args:
            # i18n: "limit" is a keyword
            lim = int(getstring(args['n'], _("limit requires a number")))
        if 'offset' in args:
            # i18n: "limit" is a keyword
            ofs = int(getstring(args['offset'], _("limit requires a number")))
        if ofs < 0:
            raise error.ParseError(_("negative offset"))
    except (TypeError, ValueError):
        # i18n: "limit" is a keyword
        raise error.ParseError(_("limit expects a number"))
    os = getset(repo, fullreposet(repo), args['set'])
    result = []
    it = iter(os)
    for x in xrange(ofs):
        y = next(it, None)
        if y is None:
            break
    for x in xrange(lim):
        y = next(it, None)
        if y is None:
            break
        elif y in subset:
            result.append(y)
    return baseset(result, datarepr=('<limit n=%d, offset=%d, %r, %r>',
                                     lim, ofs, subset, os))

@predicate('last(set, [n])', safe=True)
def last(repo, subset, x):
    """Last n members of set, defaulting to 1.
    """
    # i18n: "last" is a keyword
    l = getargs(x, 1, 2, _("last requires one or two arguments"))
    try:
        lim = 1
        if len(l) == 2:
            # i18n: "last" is a keyword
            lim = int(getstring(l[1], _("last requires a number")))
    except (TypeError, ValueError):
        # i18n: "last" is a keyword
        raise error.ParseError(_("last expects a number"))
    os = getset(repo, fullreposet(repo), l[0])
    os.reverse()
    result = []
    it = iter(os)
    for x in xrange(lim):
        y = next(it, None)
        if y is None:
            break
        elif y in subset:
            result.append(y)
    return baseset(result, datarepr=('<last n=%d, %r, %r>', lim, subset, os))

@predicate('max(set)', safe=True)
def maxrev(repo, subset, x):
    """Changeset with highest revision number in set.
    """
    os = getset(repo, fullreposet(repo), x)
    try:
        m = os.max()
        if m in subset:
            return baseset([m], datarepr=('<max %r, %r>', subset, os))
    except ValueError:
        # os.max() throws a ValueError when the collection is empty.
        # Same as python's max().
        pass
    return baseset(datarepr=('<max %r, %r>', subset, os))

@predicate('merge()', safe=True)
def merge(repo, subset, x):
    """Changeset is a merge changeset.
    """
    # i18n: "merge" is a keyword
    getargs(x, 0, 0, _("merge takes no arguments"))
    cl = repo.changelog
    return subset.filter(lambda r: cl.parentrevs(r)[1] != -1,
                         condrepr='<merge>')

@predicate('branchpoint()', safe=True)
def branchpoint(repo, subset, x):
    """Changesets with more than one child.
    """
    # i18n: "branchpoint" is a keyword
    getargs(x, 0, 0, _("branchpoint takes no arguments"))
    cl = repo.changelog
    if not subset:
        return baseset()
    # XXX this should be 'parentset.min()' assuming 'parentset' is a smartset
    # (and if it is not, it should.)
    baserev = min(subset)
    parentscount = [0]*(len(repo) - baserev)
    for r in cl.revs(start=baserev + 1):
        for p in cl.parentrevs(r):
            if p >= baserev:
                parentscount[p - baserev] += 1
    return subset.filter(lambda r: parentscount[r - baserev] > 1,
                         condrepr='<branchpoint>')

@predicate('min(set)', safe=True)
def minrev(repo, subset, x):
    """Changeset with lowest revision number in set.
    """
    os = getset(repo, fullreposet(repo), x)
    try:
        m = os.min()
        if m in subset:
            return baseset([m], datarepr=('<min %r, %r>', subset, os))
    except ValueError:
        # os.min() throws a ValueError when the collection is empty.
        # Same as python's min().
        pass
    return baseset(datarepr=('<min %r, %r>', subset, os))

@predicate('modifies(pattern)', safe=True)
def modifies(repo, subset, x):
    """Changesets modifying files matched by pattern.

    The pattern without explicit kind like ``glob:`` is expected to be
    relative to the current directory and match against a file or a
    directory.
    """
    # i18n: "modifies" is a keyword
    pat = getstring(x, _("modifies requires a pattern"))
    return checkstatus(repo, subset, pat, 0)

@predicate('named(namespace)')
def named(repo, subset, x):
    """The changesets in a given namespace.

    If `namespace` starts with `re:`, the remainder of the string is treated as
    a regular expression. To match a namespace that actually starts with `re:`,
    use the prefix `literal:`.
    """
    # i18n: "named" is a keyword
    args = getargs(x, 1, 1, _('named requires a namespace argument'))

    ns = getstring(args[0],
                   # i18n: "named" is a keyword
                   _('the argument to named must be a string'))
    kind, pattern, matcher = util.stringmatcher(ns)
    namespaces = set()
    if kind == 'literal':
        if pattern not in repo.names:
            raise error.RepoLookupError(_("namespace '%s' does not exist")
                                        % ns)
        namespaces.add(repo.names[pattern])
    else:
        for name, ns in repo.names.iteritems():
            if matcher(name):
                namespaces.add(ns)
        if not namespaces:
            raise error.RepoLookupError(_("no namespace exists"
                                          " that match '%s'") % pattern)

    names = set()
    for ns in namespaces:
        for name in ns.listnames(repo):
            if name not in ns.deprecated:
                names.update(repo[n].rev() for n in ns.nodes(repo, name))

    names -= set([node.nullrev])
    return subset & names

@predicate('id(string)', safe=True)
def node_(repo, subset, x):
    """Revision non-ambiguously specified by the given hex string prefix.
    """
    # i18n: "id" is a keyword
    l = getargs(x, 1, 1, _("id requires one argument"))
    # i18n: "id" is a keyword
    n = getstring(l[0], _("id requires a string"))
    if len(n) == 40:
        try:
            rn = repo.changelog.rev(node.bin(n))
        except (LookupError, TypeError):
            rn = None
    else:
        rn = None
        pm = repo.changelog._partialmatch(n)
        if pm is not None:
            rn = repo.changelog.rev(pm)

    if rn is None:
        return baseset()
    result = baseset([rn])
    return result & subset

@predicate('obsolete()', safe=True)
def obsolete(repo, subset, x):
    """Mutable changeset with a newer version."""
    # i18n: "obsolete" is a keyword
    getargs(x, 0, 0, _("obsolete takes no arguments"))
    obsoletes = obsmod.getrevs(repo, 'obsolete')
    return subset & obsoletes

@predicate('only(set, [set])', safe=True)
def only(repo, subset, x):
    """Changesets that are ancestors of the first set that are not ancestors
    of any other head in the repo. If a second set is specified, the result
    is ancestors of the first set that are not ancestors of the second set
    (i.e. ::<set1> - ::<set2>).
    """
    cl = repo.changelog
    # i18n: "only" is a keyword
    args = getargs(x, 1, 2, _('only takes one or two arguments'))
    include = getset(repo, fullreposet(repo), args[0])
    if len(args) == 1:
        if not include:
            return baseset()

        descendants = set(_revdescendants(repo, include, False))
        exclude = [rev for rev in cl.headrevs()
            if not rev in descendants and not rev in include]
    else:
        exclude = getset(repo, fullreposet(repo), args[1])

    results = set(cl.findmissingrevs(common=exclude, heads=include))
    # XXX we should turn this into a baseset instead of a set, smartset may do
    # some optimisations from the fact this is a baseset.
    return subset & results

@predicate('origin([set])', safe=True)
def origin(repo, subset, x):
    """
    Changesets that were specified as a source for the grafts, transplants or
    rebases that created the given revisions.  Omitting the optional set is the
    same as passing all().  If a changeset created by these operations is itself
    specified as a source for one of these operations, only the source changeset
    for the first operation is selected.
    """
    if x is not None:
        dests = getset(repo, fullreposet(repo), x)
    else:
        dests = fullreposet(repo)

    def _firstsrc(rev):
        src = _getrevsource(repo, rev)
        if src is None:
            return None

        while True:
            prev = _getrevsource(repo, src)

            if prev is None:
                return src
            src = prev

    o = set([_firstsrc(r) for r in dests])
    o -= set([None])
    # XXX we should turn this into a baseset instead of a set, smartset may do
    # some optimisations from the fact this is a baseset.
    return subset & o

@predicate('outgoing([path])', safe=True)
def outgoing(repo, subset, x):
    """Changesets not found in the specified destination repository, or the
    default push location.
    """
    # Avoid cycles.
    from . import (
        discovery,
        hg,
    )
    # i18n: "outgoing" is a keyword
    l = getargs(x, 0, 1, _("outgoing takes one or no arguments"))
    # i18n: "outgoing" is a keyword
    dest = l and getstring(l[0], _("outgoing requires a repository path")) or ''
    dest = repo.ui.expandpath(dest or 'default-push', dest or 'default')
    dest, branches = hg.parseurl(dest)
    revs, checkout = hg.addbranchrevs(repo, repo, branches, [])
    if revs:
        revs = [repo.lookup(rev) for rev in revs]
    other = hg.peer(repo, {}, dest)
    repo.ui.pushbuffer()
    outgoing = discovery.findcommonoutgoing(repo, other, onlyheads=revs)
    repo.ui.popbuffer()
    cl = repo.changelog
    o = set([cl.rev(r) for r in outgoing.missing])
    return subset & o

@predicate('p1([set])', safe=True)
def p1(repo, subset, x):
    """First parent of changesets in set, or the working directory.
    """
    if x is None:
        p = repo[x].p1().rev()
        if p >= 0:
            return subset & baseset([p])
        return baseset()

    ps = set()
    cl = repo.changelog
    for r in getset(repo, fullreposet(repo), x):
        ps.add(cl.parentrevs(r)[0])
    ps -= set([node.nullrev])
    # XXX we should turn this into a baseset instead of a set, smartset may do
    # some optimisations from the fact this is a baseset.
    return subset & ps

@predicate('p2([set])', safe=True)
def p2(repo, subset, x):
    """Second parent of changesets in set, or the working directory.
    """
    if x is None:
        ps = repo[x].parents()
        try:
            p = ps[1].rev()
            if p >= 0:
                return subset & baseset([p])
            return baseset()
        except IndexError:
            return baseset()

    ps = set()
    cl = repo.changelog
    for r in getset(repo, fullreposet(repo), x):
        ps.add(cl.parentrevs(r)[1])
    ps -= set([node.nullrev])
    # XXX we should turn this into a baseset instead of a set, smartset may do
    # some optimisations from the fact this is a baseset.
    return subset & ps

@predicate('parents([set])', safe=True)
def parents(repo, subset, x):
    """
    The set of all parents for all changesets in set, or the working directory.
    """
    if x is None:
        ps = set(p.rev() for p in repo[x].parents())
    else:
        ps = set()
        cl = repo.changelog
        up = ps.update
        parentrevs = cl.parentrevs
        for r in getset(repo, fullreposet(repo), x):
            if r == node.wdirrev:
                up(p.rev() for p in repo[r].parents())
            else:
                up(parentrevs(r))
    ps -= set([node.nullrev])
    return subset & ps

def _phase(repo, subset, target):
    """helper to select all rev in phase <target>"""
    repo._phasecache.loadphaserevs(repo) # ensure phase's sets are loaded
    if repo._phasecache._phasesets:
        s = repo._phasecache._phasesets[target] - repo.changelog.filteredrevs
        s = baseset(s)
        s.sort() # set are non ordered, so we enforce ascending
        return subset & s
    else:
        phase = repo._phasecache.phase
        condition = lambda r: phase(repo, r) == target
        return subset.filter(condition, condrepr=('<phase %r>', target),
                             cache=False)

@predicate('draft()', safe=True)
def draft(repo, subset, x):
    """Changeset in draft phase."""
    # i18n: "draft" is a keyword
    getargs(x, 0, 0, _("draft takes no arguments"))
    target = phases.draft
    return _phase(repo, subset, target)

@predicate('secret()', safe=True)
def secret(repo, subset, x):
    """Changeset in secret phase."""
    # i18n: "secret" is a keyword
    getargs(x, 0, 0, _("secret takes no arguments"))
    target = phases.secret
    return _phase(repo, subset, target)

def parentspec(repo, subset, x, n):
    """``set^0``
    The set.
    ``set^1`` (or ``set^``), ``set^2``
    First or second parent, respectively, of all changesets in set.
    """
    try:
        n = int(n[1])
        if n not in (0, 1, 2):
            raise ValueError
    except (TypeError, ValueError):
        raise error.ParseError(_("^ expects a number 0, 1, or 2"))
    ps = set()
    cl = repo.changelog
    for r in getset(repo, fullreposet(repo), x):
        if n == 0:
            ps.add(r)
        elif n == 1:
            ps.add(cl.parentrevs(r)[0])
        elif n == 2:
            parents = cl.parentrevs(r)
            if len(parents) > 1:
                ps.add(parents[1])
    return subset & ps

@predicate('present(set)', safe=True)
def present(repo, subset, x):
    """An empty set, if any revision in set isn't found; otherwise,
    all revisions in set.

    If any of specified revisions is not present in the local repository,
    the query is normally aborted. But this predicate allows the query
    to continue even in such cases.
    """
    try:
        return getset(repo, subset, x)
    except error.RepoLookupError:
        return baseset()

# for internal use
@predicate('_notpublic', safe=True)
def _notpublic(repo, subset, x):
    getargs(x, 0, 0, "_notpublic takes no arguments")
    repo._phasecache.loadphaserevs(repo) # ensure phase's sets are loaded
    if repo._phasecache._phasesets:
        s = set()
        for u in repo._phasecache._phasesets[1:]:
            s.update(u)
        s = baseset(s - repo.changelog.filteredrevs)
        s.sort()
        return subset & s
    else:
        phase = repo._phasecache.phase
        target = phases.public
        condition = lambda r: phase(repo, r) != target
        return subset.filter(condition, condrepr=('<phase %r>', target),
                             cache=False)

@predicate('public()', safe=True)
def public(repo, subset, x):
    """Changeset in public phase."""
    # i18n: "public" is a keyword
    getargs(x, 0, 0, _("public takes no arguments"))
    phase = repo._phasecache.phase
    target = phases.public
    condition = lambda r: phase(repo, r) == target
    return subset.filter(condition, condrepr=('<phase %r>', target),
                         cache=False)

@predicate('remote([id [,path]])', safe=True)
def remote(repo, subset, x):
    """Local revision that corresponds to the given identifier in a
    remote repository, if present. Here, the '.' identifier is a
    synonym for the current local branch.
    """

    from . import hg # avoid start-up nasties
    # i18n: "remote" is a keyword
    l = getargs(x, 0, 2, _("remote takes zero, one, or two arguments"))

    q = '.'
    if len(l) > 0:
    # i18n: "remote" is a keyword
        q = getstring(l[0], _("remote requires a string id"))
    if q == '.':
        q = repo['.'].branch()

    dest = ''
    if len(l) > 1:
        # i18n: "remote" is a keyword
        dest = getstring(l[1], _("remote requires a repository path"))
    dest = repo.ui.expandpath(dest or 'default')
    dest, branches = hg.parseurl(dest)
    revs, checkout = hg.addbranchrevs(repo, repo, branches, [])
    if revs:
        revs = [repo.lookup(rev) for rev in revs]
    other = hg.peer(repo, {}, dest)
    n = other.lookup(q)
    if n in repo:
        r = repo[n].rev()
        if r in subset:
            return baseset([r])
    return baseset()

@predicate('removes(pattern)', safe=True)
def removes(repo, subset, x):
    """Changesets which remove files matching pattern.

    The pattern without explicit kind like ``glob:`` is expected to be
    relative to the current directory and match against a file or a
    directory.
    """
    # i18n: "removes" is a keyword
    pat = getstring(x, _("removes requires a pattern"))
    return checkstatus(repo, subset, pat, 2)

@predicate('rev(number)', safe=True)
def rev(repo, subset, x):
    """Revision with the given numeric identifier.
    """
    # i18n: "rev" is a keyword
    l = getargs(x, 1, 1, _("rev requires one argument"))
    try:
        # i18n: "rev" is a keyword
        l = int(getstring(l[0], _("rev requires a number")))
    except (TypeError, ValueError):
        # i18n: "rev" is a keyword
        raise error.ParseError(_("rev expects a number"))
    if l not in repo.changelog and l != node.nullrev:
        return baseset()
    return subset & baseset([l])

@predicate('matching(revision [, field])', safe=True)
def matching(repo, subset, x):
    """Changesets in which a given set of fields match the set of fields in the
    selected revision or set.

    To match more than one field pass the list of fields to match separated
    by spaces (e.g. ``author description``).

    Valid fields are most regular revision fields and some special fields.

    Regular revision fields are ``description``, ``author``, ``branch``,
    ``date``, ``files``, ``phase``, ``parents``, ``substate``, ``user``
    and ``diff``.
    Note that ``author`` and ``user`` are synonyms. ``diff`` refers to the
    contents of the revision. Two revisions matching their ``diff`` will
    also match their ``files``.

    Special fields are ``summary`` and ``metadata``:
    ``summary`` matches the first line of the description.
    ``metadata`` is equivalent to matching ``description user date``
    (i.e. it matches the main metadata fields).

    ``metadata`` is the default field which is used when no fields are
    specified. You can match more than one field at a time.
    """
    # i18n: "matching" is a keyword
    l = getargs(x, 1, 2, _("matching takes 1 or 2 arguments"))

    revs = getset(repo, fullreposet(repo), l[0])

    fieldlist = ['metadata']
    if len(l) > 1:
            fieldlist = getstring(l[1],
                # i18n: "matching" is a keyword
                _("matching requires a string "
                "as its second argument")).split()

    # Make sure that there are no repeated fields,
    # expand the 'special' 'metadata' field type
    # and check the 'files' whenever we check the 'diff'
    fields = []
    for field in fieldlist:
        if field == 'metadata':
            fields += ['user', 'description', 'date']
        elif field == 'diff':
            # a revision matching the diff must also match the files
            # since matching the diff is very costly, make sure to
            # also match the files first
            fields += ['files', 'diff']
        else:
            if field == 'author':
                field = 'user'
            fields.append(field)
    fields = set(fields)
    if 'summary' in fields and 'description' in fields:
        # If a revision matches its description it also matches its summary
        fields.discard('summary')

    # We may want to match more than one field
    # Not all fields take the same amount of time to be matched
    # Sort the selected fields in order of increasing matching cost
    fieldorder = ['phase', 'parents', 'user', 'date', 'branch', 'summary',
        'files', 'description', 'substate', 'diff']
    def fieldkeyfunc(f):
        try:
            return fieldorder.index(f)
        except ValueError:
            # assume an unknown field is very costly
            return len(fieldorder)
    fields = list(fields)
    fields.sort(key=fieldkeyfunc)

    # Each field will be matched with its own "getfield" function
    # which will be added to the getfieldfuncs array of functions
    getfieldfuncs = []
    _funcs = {
        'user': lambda r: repo[r].user(),
        'branch': lambda r: repo[r].branch(),
        'date': lambda r: repo[r].date(),
        'description': lambda r: repo[r].description(),
        'files': lambda r: repo[r].files(),
        'parents': lambda r: repo[r].parents(),
        'phase': lambda r: repo[r].phase(),
        'substate': lambda r: repo[r].substate,
        'summary': lambda r: repo[r].description().splitlines()[0],
        'diff': lambda r: list(repo[r].diff(git=True),)
    }
    for info in fields:
        getfield = _funcs.get(info, None)
        if getfield is None:
            raise error.ParseError(
                # i18n: "matching" is a keyword
                _("unexpected field name passed to matching: %s") % info)
        getfieldfuncs.append(getfield)
    # convert the getfield array of functions into a "getinfo" function
    # which returns an array of field values (or a single value if there
    # is only one field to match)
    getinfo = lambda r: [f(r) for f in getfieldfuncs]

    def matches(x):
        for rev in revs:
            target = getinfo(rev)
            match = True
            for n, f in enumerate(getfieldfuncs):
                if target[n] != f(x):
                    match = False
            if match:
                return True
        return False

    return subset.filter(matches, condrepr=('<matching%r %r>', fields, revs))

@predicate('reverse(set)', safe=True)
def reverse(repo, subset, x):
    """Reverse order of set.
    """
    l = getset(repo, subset, x)
    l.reverse()
    return l

@predicate('roots(set)', safe=True)
def roots(repo, subset, x):
    """Changesets in set with no parent changeset in set.
    """
    s = getset(repo, fullreposet(repo), x)
    parents = repo.changelog.parentrevs
    def filter(r):
        for p in parents(r):
            if 0 <= p and p in s:
                return False
        return True
    return subset & s.filter(filter, condrepr='<roots>')

@predicate('sort(set[, [-]key...])', safe=True)
def sort(repo, subset, x):
    """Sort set by keys. The default sort order is ascending, specify a key
    as ``-key`` to sort in descending order.

    The keys can be:

    - ``rev`` for the revision number,
    - ``branch`` for the branch name,
    - ``desc`` for the commit message (description),
    - ``user`` for user name (``author`` can be used as an alias),
    - ``date`` for the commit date
    """
    # i18n: "sort" is a keyword
    l = getargs(x, 1, 2, _("sort requires one or two arguments"))
    keys = "rev"
    if len(l) == 2:
        # i18n: "sort" is a keyword
        keys = getstring(l[1], _("sort spec must be a string"))

    s = l[0]
    keys = keys.split()
    revs = getset(repo, subset, s)
    if keys == ["rev"]:
        revs.sort()
        return revs
    elif keys == ["-rev"]:
        revs.sort(reverse=True)
        return revs
    # sort() is guaranteed to be stable
    ctxs = [repo[r] for r in revs]
    for k in reversed(keys):
        if k == 'rev':
            ctxs.sort(key=lambda c: c.rev())
        elif k == '-rev':
            ctxs.sort(key=lambda c: c.rev(), reverse=True)
        elif k == 'branch':
            ctxs.sort(key=lambda c: c.branch())
        elif k == '-branch':
            ctxs.sort(key=lambda c: c.branch(), reverse=True)
        elif k == 'desc':
            ctxs.sort(key=lambda c: c.description())
        elif k == '-desc':
            ctxs.sort(key=lambda c: c.description(), reverse=True)
        elif k in 'user author':
            ctxs.sort(key=lambda c: c.user())
        elif k in '-user -author':
            ctxs.sort(key=lambda c: c.user(), reverse=True)
        elif k == 'date':
            ctxs.sort(key=lambda c: c.date()[0])
        elif k == '-date':
            ctxs.sort(key=lambda c: c.date()[0], reverse=True)
        else:
            raise error.ParseError(_("unknown sort key %r") % k)
    return baseset([c.rev() for c in ctxs])

@predicate('subrepo([pattern])')
def subrepo(repo, subset, x):
    """Changesets that add, modify or remove the given subrepo.  If no subrepo
    pattern is named, any subrepo changes are returned.
    """
    # i18n: "subrepo" is a keyword
    args = getargs(x, 0, 1, _('subrepo takes at most one argument'))
    pat = None
    if len(args) != 0:
        pat = getstring(args[0], _("subrepo requires a pattern"))

    m = matchmod.exact(repo.root, repo.root, ['.hgsubstate'])

    def submatches(names):
        k, p, m = util.stringmatcher(pat)
        for name in names:
            if m(name):
                yield name

    def matches(x):
        c = repo[x]
        s = repo.status(c.p1().node(), c.node(), match=m)

        if pat is None:
            return s.added or s.modified or s.removed

        if s.added:
            return any(submatches(c.substate.keys()))

        if s.modified:
            subs = set(c.p1().substate.keys())
            subs.update(c.substate.keys())

            for path in submatches(subs):
                if c.p1().substate.get(path) != c.substate.get(path):
                    return True

        if s.removed:
            return any(submatches(c.p1().substate.keys()))

        return False

    return subset.filter(matches, condrepr=('<subrepo %r>', pat))

def _substringmatcher(pattern):
    kind, pattern, matcher = util.stringmatcher(pattern)
    if kind == 'literal':
        matcher = lambda s: pattern in s
    return kind, pattern, matcher

@predicate('tag([name])', safe=True)
def tag(repo, subset, x):
    """The specified tag by name, or all tagged revisions if no name is given.

    If `name` starts with `re:`, the remainder of the name is treated as
    a regular expression. To match a tag that actually starts with `re:`,
    use the prefix `literal:`.
    """
    # i18n: "tag" is a keyword
    args = getargs(x, 0, 1, _("tag takes one or no arguments"))
    cl = repo.changelog
    if args:
        pattern = getstring(args[0],
                            # i18n: "tag" is a keyword
                            _('the argument to tag must be a string'))
        kind, pattern, matcher = util.stringmatcher(pattern)
        if kind == 'literal':
            # avoid resolving all tags
            tn = repo._tagscache.tags.get(pattern, None)
            if tn is None:
                raise error.RepoLookupError(_("tag '%s' does not exist")
                                            % pattern)
            s = set([repo[tn].rev()])
        else:
            s = set([cl.rev(n) for t, n in repo.tagslist() if matcher(t)])
    else:
        s = set([cl.rev(n) for t, n in repo.tagslist() if t != 'tip'])
    return subset & s

@predicate('tagged', safe=True)
def tagged(repo, subset, x):
    return tag(repo, subset, x)

@predicate('unstable()', safe=True)
def unstable(repo, subset, x):
    """Non-obsolete changesets with obsolete ancestors.
    """
    # i18n: "unstable" is a keyword
    getargs(x, 0, 0, _("unstable takes no arguments"))
    unstables = obsmod.getrevs(repo, 'unstable')
    return subset & unstables


@predicate('user(string)', safe=True)
def user(repo, subset, x):
    """User name contains string. The match is case-insensitive.

    If `string` starts with `re:`, the remainder of the string is treated as
    a regular expression. To match a user that actually contains `re:`, use
    the prefix `literal:`.
    """
    return author(repo, subset, x)

# experimental
@predicate('wdir', safe=True)
def wdir(repo, subset, x):
    # i18n: "wdir" is a keyword
    getargs(x, 0, 0, _("wdir takes no arguments"))
    if node.wdirrev in subset or isinstance(subset, fullreposet):
        return baseset([node.wdirrev])
    return baseset()

# for internal use
@predicate('_list', safe=True)
def _list(repo, subset, x):
    s = getstring(x, "internal error")
    if not s:
        return baseset()
    # remove duplicates here. it's difficult for caller to deduplicate sets
    # because different symbols can point to the same rev.
    cl = repo.changelog
    ls = []
    seen = set()
    for t in s.split('\0'):
        try:
            # fast path for integer revision
            r = int(t)
            if str(r) != t or r not in cl:
                raise ValueError
            revs = [r]
        except ValueError:
            revs = stringset(repo, subset, t)

        for r in revs:
            if r in seen:
                continue
            if (r in subset
                or r == node.nullrev and isinstance(subset, fullreposet)):
                ls.append(r)
            seen.add(r)
    return baseset(ls)

# for internal use
@predicate('_intlist', safe=True)
def _intlist(repo, subset, x):
    s = getstring(x, "internal error")
    if not s:
        return baseset()
    ls = [int(r) for r in s.split('\0')]
    s = subset
    return baseset([r for r in ls if r in s])

# for internal use
@predicate('_hexlist', safe=True)
def _hexlist(repo, subset, x):
    s = getstring(x, "internal error")
    if not s:
        return baseset()
    cl = repo.changelog
    ls = [cl.rev(node.bin(r)) for r in s.split('\0')]
    s = subset
    return baseset([r for r in ls if r in s])

methods = {
    "range": rangeset,
    "dagrange": dagrange,
    "string": stringset,
    "symbol": stringset,
    "and": andset,
    "or": orset,
    "not": notset,
    "difference": differenceset,
    "list": listset,
    "keyvalue": keyvaluepair,
    "func": func,
    "ancestor": ancestorspec,
    "parent": parentspec,
    "parentpost": p1,
}

def optimize(x, small):
    if x is None:
        return 0, x

    smallbonus = 1
    if small:
        smallbonus = .5

    op = x[0]
    if op == 'minus':
        return optimize(('and', x[1], ('not', x[2])), small)
    elif op == 'only':
        return optimize(('func', ('symbol', 'only'),
                         ('list', x[1], x[2])), small)
    elif op == 'onlypost':
        return optimize(('func', ('symbol', 'only'), x[1]), small)
    elif op == 'dagrangepre':
        return optimize(('func', ('symbol', 'ancestors'), x[1]), small)
    elif op == 'dagrangepost':
        return optimize(('func', ('symbol', 'descendants'), x[1]), small)
    elif op == 'rangeall':
        return optimize(('range', ('string', '0'), ('string', 'tip')), small)
    elif op == 'rangepre':
        return optimize(('range', ('string', '0'), x[1]), small)
    elif op == 'rangepost':
        return optimize(('range', x[1], ('string', 'tip')), small)
    elif op == 'negate':
        return optimize(('string',
                         '-' + getstring(x[1], _("can't negate that"))), small)
    elif op in 'string symbol negate':
        return smallbonus, x # single revisions are small
    elif op == 'and':
        wa, ta = optimize(x[1], True)
        wb, tb = optimize(x[2], True)

        # (::x and not ::y)/(not ::y and ::x) have a fast path
        def isonly(revs, bases):
            return (
                revs is not None
                and revs[0] == 'func'
                and getstring(revs[1], _('not a symbol')) == 'ancestors'
                and bases is not None
                and bases[0] == 'not'
                and bases[1][0] == 'func'
                and getstring(bases[1][1], _('not a symbol')) == 'ancestors')

        w = min(wa, wb)
        if isonly(ta, tb):
            return w, ('func', ('symbol', 'only'), ('list', ta[2], tb[1][2]))
        if isonly(tb, ta):
            return w, ('func', ('symbol', 'only'), ('list', tb[2], ta[1][2]))

        if tb is not None and tb[0] == 'not':
            return wa, ('difference', ta, tb[1])

        if wa > wb:
            return w, (op, tb, ta)
        return w, (op, ta, tb)
    elif op == 'or':
        # fast path for machine-generated expression, that is likely to have
        # lots of trivial revisions: 'a + b + c()' to '_list(a b) + c()'
        ws, ts, ss = [], [], []
        def flushss():
            if not ss:
                return
            if len(ss) == 1:
                w, t = ss[0]
            else:
                s = '\0'.join(t[1] for w, t in ss)
                y = ('func', ('symbol', '_list'), ('string', s))
                w, t = optimize(y, False)
            ws.append(w)
            ts.append(t)
            del ss[:]
        for y in x[1:]:
            w, t = optimize(y, False)
            if t is not None and (t[0] == 'string' or t[0] == 'symbol'):
                ss.append((w, t))
                continue
            flushss()
            ws.append(w)
            ts.append(t)
        flushss()
        if len(ts) == 1:
            return ws[0], ts[0] # 'or' operation is fully optimized out
        # we can't reorder trees by weight because it would change the order.
        # ("sort(a + b)" == "sort(b + a)", but "a + b" != "b + a")
        #   ts = tuple(t for w, t in sorted(zip(ws, ts), key=lambda wt: wt[0]))
        return max(ws), (op,) + tuple(ts)
    elif op == 'not':
        # Optimize not public() to _notpublic() because we have a fast version
        if x[1] == ('func', ('symbol', 'public'), None):
            newsym = ('func', ('symbol', '_notpublic'), None)
            o = optimize(newsym, not small)
            return o[0], o[1]
        else:
            o = optimize(x[1], not small)
            return o[0], (op, o[1])
    elif op == 'parentpost':
        o = optimize(x[1], small)
        return o[0], (op, o[1])
    elif op == 'group':
        return optimize(x[1], small)
    elif op in 'dagrange range parent ancestorspec':
        if op == 'parent':
            # x^:y means (x^) : y, not x ^ (:y)
            post = ('parentpost', x[1])
            if x[2][0] == 'dagrangepre':
                return optimize(('dagrange', post, x[2][1]), small)
            elif x[2][0] == 'rangepre':
                return optimize(('range', post, x[2][1]), small)

        wa, ta = optimize(x[1], small)
        wb, tb = optimize(x[2], small)
        return wa + wb, (op, ta, tb)
    elif op == 'list':
        ws, ts = zip(*(optimize(y, small) for y in x[1:]))
        return sum(ws), (op,) + ts
    elif op == 'func':
        f = getstring(x[1], _("not a symbol"))
        wa, ta = optimize(x[2], small)
        if f in ("author branch closed date desc file grep keyword "
                 "outgoing user"):
            w = 10 # slow
        elif f in "modifies adds removes":
            w = 30 # slower
        elif f == "contains":
            w = 100 # very slow
        elif f == "ancestor":
            w = 1 * smallbonus
        elif f in "reverse limit first _intlist":
            w = 0
        elif f in "sort":
            w = 10 # assume most sorts look at changelog
        else:
            w = 1
        return w + wa, (op, x[1], ta)
    return 1, x

# the set of valid characters for the initial letter of symbols in
# alias declarations and definitions
_aliassyminitletters = set(c for c in [chr(i) for i in xrange(256)]
                           if c.isalnum() or c in '._@$' or ord(c) > 127)

def _tokenizealias(program, lookup=None):
    """Parse alias declaration/definition into a stream of tokens

    This allows symbol names to use also ``$`` as an initial letter
    (for backward compatibility), and callers of this function should
    examine whether ``$`` is used also for unexpected symbols or not.
    """
    return tokenize(program, lookup=lookup,
                    syminitletters=_aliassyminitletters)

def _parsealias(spec):
    """Parse alias declaration/definition ``spec``

    >>> _parsealias('foo($1)')
    ('func', ('symbol', 'foo'), ('symbol', '$1'))
    >>> _parsealias('foo bar')
    Traceback (most recent call last):
      ...
    ParseError: ('invalid token', 4)
    """
    p = parser.parser(elements)
    tree, pos = p.parse(_tokenizealias(spec))
    if pos != len(spec):
        raise error.ParseError(_('invalid token'), pos)
    return parser.simplifyinfixops(tree, ('list', 'or'))

class _aliasrules(parser.basealiasrules):
    """Parsing and expansion rule set of revset aliases"""
    _section = _('revset alias')
    _parse = staticmethod(_parsealias)

    @staticmethod
    def _trygetfunc(tree):
        if tree[0] == 'func' and tree[1][0] == 'symbol':
            return tree[1][1], getlist(tree[2])

def expandaliases(ui, tree, showwarning=None):
    aliases = _aliasrules.buildmap(ui.configitems('revsetalias'))
    tree = _aliasrules.expand(aliases, tree)
    if showwarning:
        # warn about problematic (but not referred) aliases
        for name, alias in sorted(aliases.iteritems()):
            if alias.error and not alias.warned:
                showwarning(_('warning: %s\n') % (alias.error))
                alias.warned = True
    return tree

def foldconcat(tree):
    """Fold elements to be concatenated by `##`
    """
    if not isinstance(tree, tuple) or tree[0] in ('string', 'symbol'):
        return tree
    if tree[0] == '_concat':
        pending = [tree]
        l = []
        while pending:
            e = pending.pop()
            if e[0] == '_concat':
                pending.extend(reversed(e[1:]))
            elif e[0] in ('string', 'symbol'):
                l.append(e[1])
            else:
                msg = _("\"##\" can't concatenate \"%s\" element") % (e[0])
                raise error.ParseError(msg)
        return ('string', ''.join(l))
    else:
        return tuple(foldconcat(t) for t in tree)

def parse(spec, lookup=None):
    p = parser.parser(elements)
    tree, pos = p.parse(tokenize(spec, lookup=lookup))
    if pos != len(spec):
        raise error.ParseError(_("invalid token"), pos)
    return parser.simplifyinfixops(tree, ('list', 'or'))

def posttreebuilthook(tree, repo):
    # hook for extensions to execute code on the optimized tree
    pass

def match(ui, spec, repo=None):
    if not spec:
        raise error.ParseError(_("empty query"))
    lookup = None
    if repo:
        lookup = repo.__contains__
    tree = parse(spec, lookup)
    return _makematcher(ui, tree, repo)

def matchany(ui, specs, repo=None):
    """Create a matcher that will include any revisions matching one of the
    given specs"""
    if not specs:
        def mfunc(repo, subset=None):
            return baseset()
        return mfunc
    if not all(specs):
        raise error.ParseError(_("empty query"))
    lookup = None
    if repo:
        lookup = repo.__contains__
    if len(specs) == 1:
        tree = parse(specs[0], lookup)
    else:
        tree = ('or',) + tuple(parse(s, lookup) for s in specs)
    return _makematcher(ui, tree, repo)

def _makematcher(ui, tree, repo):
    if ui:
        tree = expandaliases(ui, tree, showwarning=ui.warn)
    tree = foldconcat(tree)
    weight, tree = optimize(tree, True)
    posttreebuilthook(tree, repo)
    def mfunc(repo, subset=None):
        if subset is None:
            subset = fullreposet(repo)
        if util.safehasattr(subset, 'isascending'):
            result = getset(repo, subset, tree)
        else:
            result = getset(repo, baseset(subset), tree)
        return result
    return mfunc

def formatspec(expr, *args):
    '''
    This is a convenience function for using revsets internally, and
    escapes arguments appropriately. Aliases are intentionally ignored
    so that intended expression behavior isn't accidentally subverted.

    Supported arguments:

    %r = revset expression, parenthesized
    %d = int(arg), no quoting
    %s = string(arg), escaped and single-quoted
    %b = arg.branch(), escaped and single-quoted
    %n = hex(arg), single-quoted
    %% = a literal '%'

    Prefixing the type with 'l' specifies a parenthesized list of that type.

    >>> formatspec('%r:: and %lr', '10 or 11', ("this()", "that()"))
    '(10 or 11):: and ((this()) or (that()))'
    >>> formatspec('%d:: and not %d::', 10, 20)
    '10:: and not 20::'
    >>> formatspec('%ld or %ld', [], [1])
    "_list('') or 1"
    >>> formatspec('keyword(%s)', 'foo\\xe9')
    "keyword('foo\\\\xe9')"
    >>> b = lambda: 'default'
    >>> b.branch = b
    >>> formatspec('branch(%b)', b)
    "branch('default')"
    >>> formatspec('root(%ls)', ['a', 'b', 'c', 'd'])
    "root(_list('a\\x00b\\x00c\\x00d'))"
    '''

    def quote(s):
        return repr(str(s))

    def argtype(c, arg):
        if c == 'd':
            return str(int(arg))
        elif c == 's':
            return quote(arg)
        elif c == 'r':
            parse(arg) # make sure syntax errors are confined
            return '(%s)' % arg
        elif c == 'n':
            return quote(node.hex(arg))
        elif c == 'b':
            return quote(arg.branch())

    def listexp(s, t):
        l = len(s)
        if l == 0:
            return "_list('')"
        elif l == 1:
            return argtype(t, s[0])
        elif t == 'd':
            return "_intlist('%s')" % "\0".join(str(int(a)) for a in s)
        elif t == 's':
            return "_list('%s')" % "\0".join(s)
        elif t == 'n':
            return "_hexlist('%s')" % "\0".join(node.hex(a) for a in s)
        elif t == 'b':
            return "_list('%s')" % "\0".join(a.branch() for a in s)

        m = l // 2
        return '(%s or %s)' % (listexp(s[:m], t), listexp(s[m:], t))

    ret = ''
    pos = 0
    arg = 0
    while pos < len(expr):
        c = expr[pos]
        if c == '%':
            pos += 1
            d = expr[pos]
            if d == '%':
                ret += d
            elif d in 'dsnbr':
                ret += argtype(d, args[arg])
                arg += 1
            elif d == 'l':
                # a list of some type
                pos += 1
                d = expr[pos]
                ret += listexp(list(args[arg]), d)
                arg += 1
            else:
                raise error.Abort('unexpected revspec format character %s' % d)
        else:
            ret += c
        pos += 1

    return ret

def prettyformat(tree):
    return parser.prettyformat(tree, ('string', 'symbol'))

def depth(tree):
    if isinstance(tree, tuple):
        return max(map(depth, tree)) + 1
    else:
        return 0

def funcsused(tree):
    if not isinstance(tree, tuple) or tree[0] in ('string', 'symbol'):
        return set()
    else:
        funcs = set()
        for s in tree[1:]:
            funcs |= funcsused(s)
        if tree[0] == 'func':
            funcs.add(tree[1][1])
        return funcs

def _formatsetrepr(r):
    """Format an optional printable representation of a set

    ========  =================================
    type(r)   example
    ========  =================================
    tuple     ('<not %r>', other)
    str       '<branch closed>'
    callable  lambda: '<branch %r>' % sorted(b)
    object    other
    ========  =================================
    """
    if r is None:
        return ''
    elif isinstance(r, tuple):
        return r[0] % r[1:]
    elif isinstance(r, str):
        return r
    elif callable(r):
        return r()
    else:
        return repr(r)

class abstractsmartset(object):

    def __nonzero__(self):
        """True if the smartset is not empty"""
        raise NotImplementedError()

    def __contains__(self, rev):
        """provide fast membership testing"""
        raise NotImplementedError()

    def __iter__(self):
        """iterate the set in the order it is supposed to be iterated"""
        raise NotImplementedError()

    # Attributes containing a function to perform a fast iteration in a given
    # direction. A smartset can have none, one, or both defined.
    #
    # Default value is None instead of a function returning None to avoid
    # initializing an iterator just for testing if a fast method exists.
    fastasc = None
    fastdesc = None

    def isascending(self):
        """True if the set will iterate in ascending order"""
        raise NotImplementedError()

    def isdescending(self):
        """True if the set will iterate in descending order"""
        raise NotImplementedError()

    @util.cachefunc
    def min(self):
        """return the minimum element in the set"""
        if self.fastasc is not None:
            for r in self.fastasc():
                return r
            raise ValueError('arg is an empty sequence')
        return min(self)

    @util.cachefunc
    def max(self):
        """return the maximum element in the set"""
        if self.fastdesc is not None:
            for r in self.fastdesc():
                return r
            raise ValueError('arg is an empty sequence')
        return max(self)

    def first(self):
        """return the first element in the set (user iteration perspective)

        Return None if the set is empty"""
        raise NotImplementedError()

    def last(self):
        """return the last element in the set (user iteration perspective)

        Return None if the set is empty"""
        raise NotImplementedError()

    def __len__(self):
        """return the length of the smartsets

        This can be expensive on smartset that could be lazy otherwise."""
        raise NotImplementedError()

    def reverse(self):
        """reverse the expected iteration order"""
        raise NotImplementedError()

    def sort(self, reverse=True):
        """get the set to iterate in an ascending or descending order"""
        raise NotImplementedError()

    def __and__(self, other):
        """Returns a new object with the intersection of the two collections.

        This is part of the mandatory API for smartset."""
        if isinstance(other, fullreposet):
            return self
        return self.filter(other.__contains__, condrepr=other, cache=False)

    def __add__(self, other):
        """Returns a new object with the union of the two collections.

        This is part of the mandatory API for smartset."""
        return addset(self, other)

    def __sub__(self, other):
        """Returns a new object with the substraction of the two collections.

        This is part of the mandatory API for smartset."""
        c = other.__contains__
        return self.filter(lambda r: not c(r), condrepr=('<not %r>', other),
                           cache=False)

    def filter(self, condition, condrepr=None, cache=True):
        """Returns this smartset filtered by condition as a new smartset.

        `condition` is a callable which takes a revision number and returns a
        boolean. Optional `condrepr` provides a printable representation of
        the given `condition`.

        This is part of the mandatory API for smartset."""
        # builtin cannot be cached. but do not needs to
        if cache and util.safehasattr(condition, 'func_code'):
            condition = util.cachefunc(condition)
        return filteredset(self, condition, condrepr)

class baseset(abstractsmartset):
    """Basic data structure that represents a revset and contains the basic
    operation that it should be able to perform.

    Every method in this class should be implemented by any smartset class.
    """
    def __init__(self, data=(), datarepr=None):
        """
        datarepr: a tuple of (format, obj, ...), a function or an object that
                  provides a printable representation of the given data.
        """
        self._ascending = None
        if not isinstance(data, list):
            if isinstance(data, set):
                self._set = data
                # set has no order we pick one for stability purpose
                self._ascending = True
            data = list(data)
        self._list = data
        self._datarepr = datarepr

    @util.propertycache
    def _set(self):
        return set(self._list)

    @util.propertycache
    def _asclist(self):
        asclist = self._list[:]
        asclist.sort()
        return asclist

    def __iter__(self):
        if self._ascending is None:
            return iter(self._list)
        elif self._ascending:
            return iter(self._asclist)
        else:
            return reversed(self._asclist)

    def fastasc(self):
        return iter(self._asclist)

    def fastdesc(self):
        return reversed(self._asclist)

    @util.propertycache
    def __contains__(self):
        return self._set.__contains__

    def __nonzero__(self):
        return bool(self._list)

    def sort(self, reverse=False):
        self._ascending = not bool(reverse)

    def reverse(self):
        if self._ascending is None:
            self._list.reverse()
        else:
            self._ascending = not self._ascending

    def __len__(self):
        return len(self._list)

    def isascending(self):
        """Returns True if the collection is ascending order, False if not.

        This is part of the mandatory API for smartset."""
        if len(self) <= 1:
            return True
        return self._ascending is not None and self._ascending

    def isdescending(self):
        """Returns True if the collection is descending order, False if not.

        This is part of the mandatory API for smartset."""
        if len(self) <= 1:
            return True
        return self._ascending is not None and not self._ascending

    def first(self):
        if self:
            if self._ascending is None:
                return self._list[0]
            elif self._ascending:
                return self._asclist[0]
            else:
                return self._asclist[-1]
        return None

    def last(self):
        if self:
            if self._ascending is None:
                return self._list[-1]
            elif self._ascending:
                return self._asclist[-1]
            else:
                return self._asclist[0]
        return None

    def __repr__(self):
        d = {None: '', False: '-', True: '+'}[self._ascending]
        s = _formatsetrepr(self._datarepr)
        if not s:
            l = self._list
            # if _list has been built from a set, it might have a different
            # order from one python implementation to another.
            # We fallback to the sorted version for a stable output.
            if self._ascending is not None:
                l = self._asclist
            s = repr(l)
        return '<%s%s %s>' % (type(self).__name__, d, s)

class filteredset(abstractsmartset):
    """Duck type for baseset class which iterates lazily over the revisions in
    the subset and contains a function which tests for membership in the
    revset
    """
    def __init__(self, subset, condition=lambda x: True, condrepr=None):
        """
        condition: a function that decide whether a revision in the subset
                   belongs to the revset or not.
        condrepr: a tuple of (format, obj, ...), a function or an object that
                  provides a printable representation of the given condition.
        """
        self._subset = subset
        self._condition = condition
        self._condrepr = condrepr

    def __contains__(self, x):
        return x in self._subset and self._condition(x)

    def __iter__(self):
        return self._iterfilter(self._subset)

    def _iterfilter(self, it):
        cond = self._condition
        for x in it:
            if cond(x):
                yield x

    @property
    def fastasc(self):
        it = self._subset.fastasc
        if it is None:
            return None
        return lambda: self._iterfilter(it())

    @property
    def fastdesc(self):
        it = self._subset.fastdesc
        if it is None:
            return None
        return lambda: self._iterfilter(it())

    def __nonzero__(self):
        fast = self.fastasc
        if fast is None:
            fast = self.fastdesc
        if fast is not None:
            it = fast()
        else:
            it = self

        for r in it:
            return True
        return False

    def __len__(self):
        # Basic implementation to be changed in future patches.
        # until this gets improved, we use generator expression
        # here, since list compr is free to call __len__ again
        # causing infinite recursion
        l = baseset(r for r in self)
        return len(l)

    def sort(self, reverse=False):
        self._subset.sort(reverse=reverse)

    def reverse(self):
        self._subset.reverse()

    def isascending(self):
        return self._subset.isascending()

    def isdescending(self):
        return self._subset.isdescending()

    def first(self):
        for x in self:
            return x
        return None

    def last(self):
        it = None
        if self.isascending():
            it = self.fastdesc
        elif self.isdescending():
            it = self.fastasc
        if it is not None:
            for x in it():
                return x
            return None #empty case
        else:
            x = None
            for x in self:
                pass
            return x

    def __repr__(self):
        xs = [repr(self._subset)]
        s = _formatsetrepr(self._condrepr)
        if s:
            xs.append(s)
        return '<%s %s>' % (type(self).__name__, ', '.join(xs))

def _iterordered(ascending, iter1, iter2):
    """produce an ordered iteration from two iterators with the same order

    The ascending is used to indicated the iteration direction.
    """
    choice = max
    if ascending:
        choice = min

    val1 = None
    val2 = None
    try:
        # Consume both iterators in an ordered way until one is empty
        while True:
            if val1 is None:
                val1 = iter1.next()
            if val2 is None:
                val2 = iter2.next()
            next = choice(val1, val2)
            yield next
            if val1 == next:
                val1 = None
            if val2 == next:
                val2 = None
    except StopIteration:
        # Flush any remaining values and consume the other one
        it = iter2
        if val1 is not None:
            yield val1
            it = iter1
        elif val2 is not None:
            # might have been equality and both are empty
            yield val2
        for val in it:
            yield val

class addset(abstractsmartset):
    """Represent the addition of two sets

    Wrapper structure for lazily adding two structures without losing much
    performance on the __contains__ method

    If the ascending attribute is set, that means the two structures are
    ordered in either an ascending or descending way. Therefore, we can add
    them maintaining the order by iterating over both at the same time

    >>> xs = baseset([0, 3, 2])
    >>> ys = baseset([5, 2, 4])

    >>> rs = addset(xs, ys)
    >>> bool(rs), 0 in rs, 1 in rs, 5 in rs, rs.first(), rs.last()
    (True, True, False, True, 0, 4)
    >>> rs = addset(xs, baseset([]))
    >>> bool(rs), 0 in rs, 1 in rs, rs.first(), rs.last()
    (True, True, False, 0, 2)
    >>> rs = addset(baseset([]), baseset([]))
    >>> bool(rs), 0 in rs, rs.first(), rs.last()
    (False, False, None, None)

    iterate unsorted:
    >>> rs = addset(xs, ys)
    >>> # (use generator because pypy could call len())
    >>> list(x for x in rs)  # without _genlist
    [0, 3, 2, 5, 4]
    >>> assert not rs._genlist
    >>> len(rs)
    5
    >>> [x for x in rs]  # with _genlist
    [0, 3, 2, 5, 4]
    >>> assert rs._genlist

    iterate ascending:
    >>> rs = addset(xs, ys, ascending=True)
    >>> # (use generator because pypy could call len())
    >>> list(x for x in rs), list(x for x in rs.fastasc())  # without _asclist
    ([0, 2, 3, 4, 5], [0, 2, 3, 4, 5])
    >>> assert not rs._asclist
    >>> len(rs)
    5
    >>> [x for x in rs], [x for x in rs.fastasc()]
    ([0, 2, 3, 4, 5], [0, 2, 3, 4, 5])
    >>> assert rs._asclist

    iterate descending:
    >>> rs = addset(xs, ys, ascending=False)
    >>> # (use generator because pypy could call len())
    >>> list(x for x in rs), list(x for x in rs.fastdesc())  # without _asclist
    ([5, 4, 3, 2, 0], [5, 4, 3, 2, 0])
    >>> assert not rs._asclist
    >>> len(rs)
    5
    >>> [x for x in rs], [x for x in rs.fastdesc()]
    ([5, 4, 3, 2, 0], [5, 4, 3, 2, 0])
    >>> assert rs._asclist

    iterate ascending without fastasc:
    >>> rs = addset(xs, generatorset(ys), ascending=True)
    >>> assert rs.fastasc is None
    >>> [x for x in rs]
    [0, 2, 3, 4, 5]

    iterate descending without fastdesc:
    >>> rs = addset(generatorset(xs), ys, ascending=False)
    >>> assert rs.fastdesc is None
    >>> [x for x in rs]
    [5, 4, 3, 2, 0]
    """
    def __init__(self, revs1, revs2, ascending=None):
        self._r1 = revs1
        self._r2 = revs2
        self._iter = None
        self._ascending = ascending
        self._genlist = None
        self._asclist = None

    def __len__(self):
        return len(self._list)

    def __nonzero__(self):
        return bool(self._r1) or bool(self._r2)

    @util.propertycache
    def _list(self):
        if not self._genlist:
            self._genlist = baseset(iter(self))
        return self._genlist

    def __iter__(self):
        """Iterate over both collections without repeating elements

        If the ascending attribute is not set, iterate over the first one and
        then over the second one checking for membership on the first one so we
        dont yield any duplicates.

        If the ascending attribute is set, iterate over both collections at the
        same time, yielding only one value at a time in the given order.
        """
        if self._ascending is None:
            if self._genlist:
                return iter(self._genlist)
            def arbitraryordergen():
                for r in self._r1:
                    yield r
                inr1 = self._r1.__contains__
                for r in self._r2:
                    if not inr1(r):
                        yield r
            return arbitraryordergen()
        # try to use our own fast iterator if it exists
        self._trysetasclist()
        if self._ascending:
            attr = 'fastasc'
        else:
            attr = 'fastdesc'
        it = getattr(self, attr)
        if it is not None:
            return it()
        # maybe half of the component supports fast
        # get iterator for _r1
        iter1 = getattr(self._r1, attr)
        if iter1 is None:
            # let's avoid side effect (not sure it matters)
            iter1 = iter(sorted(self._r1, reverse=not self._ascending))
        else:
            iter1 = iter1()
        # get iterator for _r2
        iter2 = getattr(self._r2, attr)
        if iter2 is None:
            # let's avoid side effect (not sure it matters)
            iter2 = iter(sorted(self._r2, reverse=not self._ascending))
        else:
            iter2 = iter2()
        return _iterordered(self._ascending, iter1, iter2)

    def _trysetasclist(self):
        """populate the _asclist attribute if possible and necessary"""
        if self._genlist is not None and self._asclist is None:
            self._asclist = sorted(self._genlist)

    @property
    def fastasc(self):
        self._trysetasclist()
        if self._asclist is not None:
            return self._asclist.__iter__
        iter1 = self._r1.fastasc
        iter2 = self._r2.fastasc
        if None in (iter1, iter2):
            return None
        return lambda: _iterordered(True, iter1(), iter2())

    @property
    def fastdesc(self):
        self._trysetasclist()
        if self._asclist is not None:
            return self._asclist.__reversed__
        iter1 = self._r1.fastdesc
        iter2 = self._r2.fastdesc
        if None in (iter1, iter2):
            return None
        return lambda: _iterordered(False, iter1(), iter2())

    def __contains__(self, x):
        return x in self._r1 or x in self._r2

    def sort(self, reverse=False):
        """Sort the added set

        For this we use the cached list with all the generated values and if we
        know they are ascending or descending we can sort them in a smart way.
        """
        self._ascending = not reverse

    def isascending(self):
        return self._ascending is not None and self._ascending

    def isdescending(self):
        return self._ascending is not None and not self._ascending

    def reverse(self):
        if self._ascending is None:
            self._list.reverse()
        else:
            self._ascending = not self._ascending

    def first(self):
        for x in self:
            return x
        return None

    def last(self):
        self.reverse()
        val = self.first()
        self.reverse()
        return val

    def __repr__(self):
        d = {None: '', False: '-', True: '+'}[self._ascending]
        return '<%s%s %r, %r>' % (type(self).__name__, d, self._r1, self._r2)

class generatorset(abstractsmartset):
    """Wrap a generator for lazy iteration

    Wrapper structure for generators that provides lazy membership and can
    be iterated more than once.
    When asked for membership it generates values until either it finds the
    requested one or has gone through all the elements in the generator
    """
    def __init__(self, gen, iterasc=None):
        """
        gen: a generator producing the values for the generatorset.
        """
        self._gen = gen
        self._asclist = None
        self._cache = {}
        self._genlist = []
        self._finished = False
        self._ascending = True
        if iterasc is not None:
            if iterasc:
                self.fastasc = self._iterator
                self.__contains__ = self._asccontains
            else:
                self.fastdesc = self._iterator
                self.__contains__ = self._desccontains

    def __nonzero__(self):
        # Do not use 'for r in self' because it will enforce the iteration
        # order (default ascending), possibly unrolling a whole descending
        # iterator.
        if self._genlist:
            return True
        for r in self._consumegen():
            return True
        return False

    def __contains__(self, x):
        if x in self._cache:
            return self._cache[x]

        # Use new values only, as existing values would be cached.
        for l in self._consumegen():
            if l == x:
                return True

        self._cache[x] = False
        return False

    def _asccontains(self, x):
        """version of contains optimised for ascending generator"""
        if x in self._cache:
            return self._cache[x]

        # Use new values only, as existing values would be cached.
        for l in self._consumegen():
            if l == x:
                return True
            if l > x:
                break

        self._cache[x] = False
        return False

    def _desccontains(self, x):
        """version of contains optimised for descending generator"""
        if x in self._cache:
            return self._cache[x]

        # Use new values only, as existing values would be cached.
        for l in self._consumegen():
            if l == x:
                return True
            if l < x:
                break

        self._cache[x] = False
        return False

    def __iter__(self):
        if self._ascending:
            it = self.fastasc
        else:
            it = self.fastdesc
        if it is not None:
            return it()
        # we need to consume the iterator
        for x in self._consumegen():
            pass
        # recall the same code
        return iter(self)

    def _iterator(self):
        if self._finished:
            return iter(self._genlist)

        # We have to use this complex iteration strategy to allow multiple
        # iterations at the same time. We need to be able to catch revision
        # removed from _consumegen and added to genlist in another instance.
        #
        # Getting rid of it would provide an about 15% speed up on this
        # iteration.
        genlist = self._genlist
        nextrev = self._consumegen().next
        _len = len # cache global lookup
        def gen():
            i = 0
            while True:
                if i < _len(genlist):
                    yield genlist[i]
                else:
                    yield nextrev()
                i += 1
        return gen()

    def _consumegen(self):
        cache = self._cache
        genlist = self._genlist.append
        for item in self._gen:
            cache[item] = True
            genlist(item)
            yield item
        if not self._finished:
            self._finished = True
            asc = self._genlist[:]
            asc.sort()
            self._asclist = asc
            self.fastasc = asc.__iter__
            self.fastdesc = asc.__reversed__

    def __len__(self):
        for x in self._consumegen():
            pass
        return len(self._genlist)

    def sort(self, reverse=False):
        self._ascending = not reverse

    def reverse(self):
        self._ascending = not self._ascending

    def isascending(self):
        return self._ascending

    def isdescending(self):
        return not self._ascending

    def first(self):
        if self._ascending:
            it = self.fastasc
        else:
            it = self.fastdesc
        if it is None:
            # we need to consume all and try again
            for x in self._consumegen():
                pass
            return self.first()
        return next(it(), None)

    def last(self):
        if self._ascending:
            it = self.fastdesc
        else:
            it = self.fastasc
        if it is None:
            # we need to consume all and try again
            for x in self._consumegen():
                pass
            return self.first()
        return next(it(), None)

    def __repr__(self):
        d = {False: '-', True: '+'}[self._ascending]
        return '<%s%s>' % (type(self).__name__, d)

class spanset(abstractsmartset):
    """Duck type for baseset class which represents a range of revisions and
    can work lazily and without having all the range in memory

    Note that spanset(x, y) behave almost like xrange(x, y) except for two
    notable points:
    - when x < y it will be automatically descending,
    - revision filtered with this repoview will be skipped.

    """
    def __init__(self, repo, start=0, end=None):
        """
        start: first revision included the set
               (default to 0)
        end:   first revision excluded (last+1)
               (default to len(repo)

        Spanset will be descending if `end` < `start`.
        """
        if end is None:
            end = len(repo)
        self._ascending = start <= end
        if not self._ascending:
            start, end = end + 1, start +1
        self._start = start
        self._end = end
        self._hiddenrevs = repo.changelog.filteredrevs

    def sort(self, reverse=False):
        self._ascending = not reverse

    def reverse(self):
        self._ascending = not self._ascending

    def _iterfilter(self, iterrange):
        s = self._hiddenrevs
        for r in iterrange:
            if r not in s:
                yield r

    def __iter__(self):
        if self._ascending:
            return self.fastasc()
        else:
            return self.fastdesc()

    def fastasc(self):
        iterrange = xrange(self._start, self._end)
        if self._hiddenrevs:
            return self._iterfilter(iterrange)
        return iter(iterrange)

    def fastdesc(self):
        iterrange = xrange(self._end - 1, self._start - 1, -1)
        if self._hiddenrevs:
            return self._iterfilter(iterrange)
        return iter(iterrange)

    def __contains__(self, rev):
        hidden = self._hiddenrevs
        return ((self._start <= rev < self._end)
                and not (hidden and rev in hidden))

    def __nonzero__(self):
        for r in self:
            return True
        return False

    def __len__(self):
        if not self._hiddenrevs:
            return abs(self._end - self._start)
        else:
            count = 0
            start = self._start
            end = self._end
            for rev in self._hiddenrevs:
                if (end < rev <= start) or (start <= rev < end):
                    count += 1
            return abs(self._end - self._start) - count

    def isascending(self):
        return self._ascending

    def isdescending(self):
        return not self._ascending

    def first(self):
        if self._ascending:
            it = self.fastasc
        else:
            it = self.fastdesc
        for x in it():
            return x
        return None

    def last(self):
        if self._ascending:
            it = self.fastdesc
        else:
            it = self.fastasc
        for x in it():
            return x
        return None

    def __repr__(self):
        d = {False: '-', True: '+'}[self._ascending]
        return '<%s%s %d:%d>' % (type(self).__name__, d,
                                 self._start, self._end - 1)

class fullreposet(spanset):
    """a set containing all revisions in the repo

    This class exists to host special optimization and magic to handle virtual
    revisions such as "null".
    """

    def __init__(self, repo):
        super(fullreposet, self).__init__(repo)

    def __and__(self, other):
        """As self contains the whole repo, all of the other set should also be
        in self. Therefore `self & other = other`.

        This boldly assumes the other contains valid revs only.
        """
        # other not a smartset, make is so
        if not util.safehasattr(other, 'isascending'):
            # filter out hidden revision
            # (this boldly assumes all smartset are pure)
            #
            # `other` was used with "&", let's assume this is a set like
            # object.
            other = baseset(other - self._hiddenrevs)

        # XXX As fullreposet is also used as bootstrap, this is wrong.
        #
        # With a giveme312() revset returning [3,1,2], this makes
        #   'hg log -r "giveme312()"' -> 1, 2, 3 (wrong)
        # We cannot just drop it because other usage still need to sort it:
        #   'hg log -r "all() and giveme312()"' -> 1, 2, 3 (right)
        #
        # There is also some faulty revset implementations that rely on it
        # (eg: children as of its state in e8075329c5fb)
        #
        # When we fix the two points above we can move this into the if clause
        other.sort(reverse=self.isdescending())
        return other

def prettyformatset(revs):
    lines = []
    rs = repr(revs)
    p = 0
    while p < len(rs):
        q = rs.find('<', p + 1)
        if q < 0:
            q = len(rs)
        l = rs.count('<', 0, p) - rs.count('>', 0, p)
        assert l >= 0
        lines.append((l, rs[p:q].rstrip()))
        p = q
    return '\n'.join('  ' * l + s for l, s in lines)

def loadpredicate(ui, extname, registrarobj):
    """Load revset predicates from specified registrarobj
    """
    for name, func in registrarobj._table.iteritems():
        symbols[name] = func
        if func._safe:
            safesymbols.add(name)

# load built-in predicates explicitly to setup safesymbols
loadpredicate(None, None, predicate)

# tell hggettext to extract docstrings from these functions:
i18nfunctions = symbols.values()
