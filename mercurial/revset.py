# revset.py - revision set queries for mercurial
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import re
import parser, util, error, discovery
import bookmarks as bookmarksmod
import match as matchmod
from i18n import _, gettext

elements = {
    "(": (20, ("group", 1, ")"), ("func", 1, ")")),
    "-": (5, ("negate", 19), ("minus", 5)),
    "::": (17, ("dagrangepre", 17), ("dagrange", 17),
           ("dagrangepost", 17)),
    "..": (17, ("dagrangepre", 17), ("dagrange", 17),
           ("dagrangepost", 17)),
    ":": (15, ("rangepre", 15), ("range", 15), ("rangepost", 15)),
    "not": (10, ("not", 10)),
    "!": (10, ("not", 10)),
    "and": (5, None, ("and", 5)),
    "&": (5, None, ("and", 5)),
    "or": (4, None, ("or", 4)),
    "|": (4, None, ("or", 4)),
    "+": (4, None, ("or", 4)),
    ",": (2, None, ("list", 2)),
    ")": (0, None, None),
    "symbol": (0, ("symbol",), None),
    "string": (0, ("string",), None),
    "end": (0, None, None),
}

keywords = set(['and', 'or', 'not'])

def tokenize(program):
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
        elif c in "():,-|&+!": # handle simple operators
            yield (c, None, pos)
        elif (c in '"\'' or c == 'r' and
              program[pos:pos + 2] in ("r'", 'r"')): # handle quoted strings
            if c == 'r':
                pos += 1
                c = program[pos]
                decode = lambda x: x
            else:
                decode = lambda x: x.decode('string-escape')
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
        elif c.isalnum() or c in '._' or ord(c) > 127: # gather up a symbol/keyword
            s = pos
            pos += 1
            while pos < l: # find end of symbol
                d = program[pos]
                if not (d.isalnum() or d in "._" or ord(d) > 127):
                    break
                if d == '.' and program[pos - 1] == '.': # special case for ..
                    pos -= 1
                    break
                pos += 1
            sym = program[s:pos]
            if sym in keywords: # operator keywords
                yield (sym, None, s)
            else:
                yield ('symbol', sym, s)
            pos -= 1
        else:
            raise error.ParseError(_("syntax error"), pos)
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
        return getlist(x[1]) + [x[2]]
    return [x]

def getargs(x, min, max, err):
    l = getlist(x)
    if len(l) < min or len(l) > max:
        raise error.ParseError(err)
    return l

def getset(repo, subset, x):
    if not x:
        raise error.ParseError(_("missing argument"))
    return methods[x[0]](repo, subset, *x[1:])

# operator methods

def stringset(repo, subset, x):
    x = repo[x].rev()
    if x == -1 and len(subset) == len(repo):
        return [-1]
    if x in subset:
        return [x]
    return []

def symbolset(repo, subset, x):
    if x in symbols:
        raise error.ParseError(_("can't use %s here") % x)
    return stringset(repo, subset, x)

def rangeset(repo, subset, x, y):
    m = getset(repo, subset, x)
    if not m:
        m = getset(repo, range(len(repo)), x)

    n = getset(repo, subset, y)
    if not n:
        n = getset(repo, range(len(repo)), y)

    if not m or not n:
        return []
    m, n = m[0], n[-1]

    if m < n:
        r = range(m, n + 1)
    else:
        r = range(m, n - 1, -1)
    s = set(subset)
    return [x for x in r if x in s]

def andset(repo, subset, x, y):
    return getset(repo, getset(repo, subset, x), y)

def orset(repo, subset, x, y):
    s = set(getset(repo, subset, x))
    s |= set(getset(repo, [r for r in subset if r not in s], y))
    return [r for r in subset if r in s]

def notset(repo, subset, x):
    s = set(getset(repo, subset, x))
    return [r for r in subset if r not in s]

def listset(repo, subset, a, b):
    raise error.ParseError(_("can't use a list in this context"))

def func(repo, subset, a, b):
    if a[0] == 'symbol' and a[1] in symbols:
        return symbols[a[1]](repo, subset, b)
    raise error.ParseError(_("not a function: %s") % a[1])

# functions

def node(repo, subset, x):
    """``id(string)``
    Revision non-ambiguously specified by the given hex string prefix.
    """
    # i18n: "id" is a keyword
    l = getargs(x, 1, 1, _("id requires one argument"))
    # i18n: "id" is a keyword
    n = getstring(l[0], _("id requires a string"))
    if len(n) == 40:
        rn = repo[n].rev()
    else:
        rn = repo.changelog.rev(repo.changelog._partialmatch(n))
    return [r for r in subset if r == rn]

def rev(repo, subset, x):
    """``rev(number)``
    Revision with the given numeric identifier.
    """
    # i18n: "rev" is a keyword
    l = getargs(x, 1, 1, _("rev requires one argument"))
    try:
        # i18n: "rev" is a keyword
        l = int(getstring(l[0], _("rev requires a number")))
    except ValueError:
        # i18n: "rev" is a keyword
        raise error.ParseError(_("rev expects a number"))
    return [r for r in subset if r == l]

def p1(repo, subset, x):
    """``p1([set])``
    First parent of changesets in set, or the working directory.
    """
    if x is None:
        p = repo[x].parents()[0].rev()
        return [r for r in subset if r == p]

    ps = set()
    cl = repo.changelog
    for r in getset(repo, range(len(repo)), x):
        ps.add(cl.parentrevs(r)[0])
    return [r for r in subset if r in ps]

def p2(repo, subset, x):
    """``p2([set])``
    Second parent of changesets in set, or the working directory.
    """
    if x is None:
        ps = repo[x].parents()
        try:
            p = ps[1].rev()
            return [r for r in subset if r == p]
        except IndexError:
            return []

    ps = set()
    cl = repo.changelog
    for r in getset(repo, range(len(repo)), x):
        ps.add(cl.parentrevs(r)[1])
    return [r for r in subset if r in ps]

def parents(repo, subset, x):
    """``parents([set])``
    The set of all parents for all changesets in set, or the working directory.
    """
    if x is None:
        ps = tuple(p.rev() for p in repo[x].parents())
        return [r for r in subset if r in ps]

    ps = set()
    cl = repo.changelog
    for r in getset(repo, range(len(repo)), x):
        ps.update(cl.parentrevs(r))
    return [r for r in subset if r in ps]

def maxrev(repo, subset, x):
    """``max(set)``
    Changeset with highest revision number in set.
    """
    s = getset(repo, subset, x)
    if s:
        m = max(s)
        if m in subset:
            return [m]
    return []

def minrev(repo, subset, x):
    """``min(set)``
    Changeset with lowest revision number in set.
    """
    s = getset(repo, subset, x)
    if s:
        m = min(s)
        if m in subset:
            return [m]
    return []

def limit(repo, subset, x):
    """``limit(set, n)``
    First n members of set.
    """
    # i18n: "limit" is a keyword
    l = getargs(x, 2, 2, _("limit requires two arguments"))
    try:
        # i18n: "limit" is a keyword
        lim = int(getstring(l[1], _("limit requires a number")))
    except ValueError:
        # i18n: "limit" is a keyword
        raise error.ParseError(_("limit expects a number"))
    return getset(repo, subset, l[0])[:lim]

def children(repo, subset, x):
    """``children(set)``
    Child changesets of changesets in set.
    """
    cs = set()
    cl = repo.changelog
    s = set(getset(repo, range(len(repo)), x))
    for r in xrange(0, len(repo)):
        for p in cl.parentrevs(r):
            if p in s:
                cs.add(r)
    return [r for r in subset if r in cs]

def branch(repo, subset, x):
    """``branch(set)``
    All changesets belonging to the branches of changesets in set.
    """
    s = getset(repo, range(len(repo)), x)
    b = set()
    for r in s:
        b.add(repo[r].branch())
    s = set(s)
    return [r for r in subset if r in s or repo[r].branch() in b]

def ancestor(repo, subset, x):
    """``ancestor(single, single)``
    Greatest common ancestor of the two changesets.
    """
    # i18n: "ancestor" is a keyword
    l = getargs(x, 2, 2, _("ancestor requires two arguments"))
    r = range(len(repo))
    a = getset(repo, r, l[0])
    b = getset(repo, r, l[1])
    if len(a) != 1 or len(b) != 1:
        # i18n: "ancestor" is a keyword
        raise error.ParseError(_("ancestor arguments must be single revisions"))
    an = [repo[a[0]].ancestor(repo[b[0]]).rev()]

    return [r for r in an if r in subset]

def ancestors(repo, subset, x):
    """``ancestors(set)``
    Changesets that are ancestors of a changeset in set.
    """
    args = getset(repo, range(len(repo)), x)
    if not args:
        return []
    s = set(repo.changelog.ancestors(*args)) | set(args)
    return [r for r in subset if r in s]

def descendants(repo, subset, x):
    """``descendants(set)``
    Changesets which are descendants of changesets in set.
    """
    args = getset(repo, range(len(repo)), x)
    if not args:
        return []
    s = set(repo.changelog.descendants(*args)) | set(args)
    return [r for r in subset if r in s]

def follow(repo, subset, x):
    """``follow()``
    An alias for ``::.`` (ancestors of the working copy's first parent).
    """
    # i18n: "follow" is a keyword
    getargs(x, 0, 0, _("follow takes no arguments"))
    p = repo['.'].rev()
    s = set(repo.changelog.ancestors(p)) | set([p])
    return [r for r in subset if r in s]

def date(repo, subset, x):
    """``date(interval)``
    Changesets within the interval, see :hg:`help dates`.
    """
    # i18n: "date" is a keyword
    ds = getstring(x, _("date requires a string"))
    dm = util.matchdate(ds)
    return [r for r in subset if dm(repo[r].date()[0])]

def keyword(repo, subset, x):
    """``keyword(string)``
    Search commit message, user name, and names of changed files for
    string.
    """
    # i18n: "keyword" is a keyword
    kw = getstring(x, _("keyword requires a string")).lower()
    l = []
    for r in subset:
        c = repo[r]
        t = " ".join(c.files() + [c.user(), c.description()])
        if kw in t.lower():
            l.append(r)
    return l

def grep(repo, subset, x):
    """``grep(regex)``
    Like ``keyword(string)`` but accepts a regex. Use ``grep(r'...')``
    to ensure special escape characters are handled correctly.
    """
    try:
        # i18n: "grep" is a keyword
        gr = re.compile(getstring(x, _("grep requires a string")))
    except re.error, e:
        raise error.ParseError(_('invalid match pattern: %s') % e)
    l = []
    for r in subset:
        c = repo[r]
        for e in c.files() + [c.user(), c.description()]:
            if gr.search(e):
                l.append(r)
                continue
    return l

def author(repo, subset, x):
    """``author(string)``
    Alias for ``user(string)``.
    """
    # i18n: "author" is a keyword
    n = getstring(x, _("author requires a string")).lower()
    return [r for r in subset if n in repo[r].user().lower()]

def user(repo, subset, x):
    """``user(string)``
    User name is string.
    """
    return author(repo, subset, x)

def hasfile(repo, subset, x):
    """``file(pattern)``
    Changesets affecting files matched by pattern.
    """
    # i18n: "file" is a keyword
    pat = getstring(x, _("file requires a pattern"))
    m = matchmod.match(repo.root, repo.getcwd(), [pat])
    s = []
    for r in subset:
        for f in repo[r].files():
            if m(f):
                s.append(r)
                continue
    return s

def contains(repo, subset, x):
    """``contains(pattern)``
    Revision contains pattern.
    """
    # i18n: "contains" is a keyword
    pat = getstring(x, _("contains requires a pattern"))
    m = matchmod.match(repo.root, repo.getcwd(), [pat])
    s = []
    if m.files() == [pat]:
        for r in subset:
            if pat in repo[r]:
                s.append(r)
                continue
    else:
        for r in subset:
            for f in repo[r].manifest():
                if m(f):
                    s.append(r)
                    continue
    return s

def checkstatus(repo, subset, pat, field):
    m = matchmod.match(repo.root, repo.getcwd(), [pat])
    s = []
    fast = (m.files() == [pat])
    for r in subset:
        c = repo[r]
        if fast:
            if pat not in c.files():
                continue
        else:
            for f in c.files():
                if m(f):
                    break
            else:
                continue
        files = repo.status(c.p1().node(), c.node())[field]
        if fast:
            if pat in files:
                s.append(r)
                continue
        else:
            for f in files:
                if m(f):
                    s.append(r)
                    continue
    return s

def modifies(repo, subset, x):
    """``modifies(pattern)``
    Changesets modifying files matched by pattern.
    """
    # i18n: "modifies" is a keyword
    pat = getstring(x, _("modifies requires a pattern"))
    return checkstatus(repo, subset, pat, 0)

def adds(repo, subset, x):
    """``adds(pattern)``
    Changesets that add a file matching pattern.
    """
    # i18n: "adds" is a keyword
    pat = getstring(x, _("adds requires a pattern"))
    return checkstatus(repo, subset, pat, 1)

def removes(repo, subset, x):
    """``removes(pattern)``
    Changesets which remove files matching pattern.
    """
    # i18n: "removes" is a keyword
    pat = getstring(x, _("removes requires a pattern"))
    return checkstatus(repo, subset, pat, 2)

def merge(repo, subset, x):
    """``merge()``
    Changeset is a merge changeset.
    """
    # i18n: "merge" is a keyword
    getargs(x, 0, 0, _("merge takes no arguments"))
    cl = repo.changelog
    return [r for r in subset if cl.parentrevs(r)[1] != -1]

def closed(repo, subset, x):
    """``closed()``
    Changeset is closed.
    """
    # i18n: "closed" is a keyword
    getargs(x, 0, 0, _("closed takes no arguments"))
    return [r for r in subset if repo[r].extra().get('close')]

def head(repo, subset, x):
    """``head()``
    Changeset is a named branch head.
    """
    # i18n: "head" is a keyword
    getargs(x, 0, 0, _("head takes no arguments"))
    hs = set()
    for b, ls in repo.branchmap().iteritems():
        hs.update(repo[h].rev() for h in ls)
    return [r for r in subset if r in hs]

def reverse(repo, subset, x):
    """``reverse(set)``
    Reverse order of set.
    """
    l = getset(repo, subset, x)
    l.reverse()
    return l

def present(repo, subset, x):
    """``present(set)``
    An empty set, if any revision in set isn't found; otherwise,
    all revisions in set.
    """
    try:
        return getset(repo, subset, x)
    except error.RepoLookupError:
        return []

def sort(repo, subset, x):
    """``sort(set[, [-]key...])``
    Sort set by keys. The default sort order is ascending, specify a key
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
        keys = getstring(l[1], _("sort spec must be a string"))

    s = l[0]
    keys = keys.split()
    l = []
    def invert(s):
        return "".join(chr(255 - ord(c)) for c in s)
    for r in getset(repo, subset, s):
        c = repo[r]
        e = []
        for k in keys:
            if k == 'rev':
                e.append(r)
            elif k == '-rev':
                e.append(-r)
            elif k == 'branch':
                e.append(c.branch())
            elif k == '-branch':
                e.append(invert(c.branch()))
            elif k == 'desc':
                e.append(c.description())
            elif k == '-desc':
                e.append(invert(c.description()))
            elif k in 'user author':
                e.append(c.user())
            elif k in '-user -author':
                e.append(invert(c.user()))
            elif k == 'date':
                e.append(c.date()[0])
            elif k == '-date':
                e.append(-c.date()[0])
            else:
                raise error.ParseError(_("unknown sort key %r") % k)
        e.append(r)
        l.append(e)
    l.sort()
    return [e[-1] for e in l]

def getall(repo, subset, x):
    """``all()``
    All changesets, the same as ``0:tip``.
    """
    # i18n: "all" is a keyword
    getargs(x, 0, 0, _("all takes no arguments"))
    return subset

def heads(repo, subset, x):
    """``heads(set)``
    Members of set with no children in set.
    """
    s = getset(repo, subset, x)
    ps = set(parents(repo, subset, x))
    return [r for r in s if r not in ps]

def roots(repo, subset, x):
    """``roots(set)``
    Changesets with no parent changeset in set.
    """
    s = getset(repo, subset, x)
    cs = set(children(repo, subset, x))
    return [r for r in s if r not in cs]

def outgoing(repo, subset, x):
    """``outgoing([path])``
    Changesets not found in the specified destination repository, or the
    default push location.
    """
    import hg # avoid start-up nasties
    # i18n: "outgoing" is a keyword
    l = getargs(x, 0, 1, _("outgoing requires a repository path"))
    # i18n: "outgoing" is a keyword
    dest = l and getstring(l[0], _("outgoing requires a repository path")) or ''
    dest = repo.ui.expandpath(dest or 'default-push', dest or 'default')
    dest, branches = hg.parseurl(dest)
    revs, checkout = hg.addbranchrevs(repo, repo, branches, [])
    if revs:
        revs = [repo.lookup(rev) for rev in revs]
    other = hg.repository(hg.remoteui(repo, {}), dest)
    repo.ui.pushbuffer()
    o = discovery.findoutgoing(repo, other)
    repo.ui.popbuffer()
    cl = repo.changelog
    o = set([cl.rev(r) for r in repo.changelog.nodesbetween(o, revs)[0]])
    return [r for r in subset if r in o]

def tag(repo, subset, x):
    """``tag(name)``
    The specified tag by name, or all tagged revisions if no name is given.
    """
    # i18n: "tag" is a keyword
    args = getargs(x, 0, 1, _("tag takes one or no arguments"))
    cl = repo.changelog
    if args:
        tn = getstring(args[0],
                       # i18n: "tag" is a keyword
                       _('the argument to tag must be a string'))
        s = set([cl.rev(n) for t, n in repo.tagslist() if t == tn])
    else:
        s = set([cl.rev(n) for t, n in repo.tagslist() if t != 'tip'])
    return [r for r in subset if r in s]

def tagged(repo, subset, x):
    return tag(repo, subset, x)

def bookmark(repo, subset, x):
    """``bookmark([name])``
    The named bookmark or all bookmarks.
    """
    # i18n: "bookmark" is a keyword
    args = getargs(x, 0, 1, _('bookmark takes one or no arguments'))
    if args:
        bm = getstring(args[0],
                       # i18n: "bookmark" is a keyword
                       _('the argument to bookmark must be a string'))
        bmrev = bookmarksmod.listbookmarks(repo).get(bm, None)
        if bmrev:
            bmrev = repo[bmrev].rev()
        return [r for r in subset if r == bmrev]
    bms = set([repo[r].rev()
               for r in bookmarksmod.listbookmarks(repo).values()])
    return [r for r in subset if r in bms]

symbols = {
    "adds": adds,
    "all": getall,
    "ancestor": ancestor,
    "ancestors": ancestors,
    "author": author,
    "bookmark": bookmark,
    "branch": branch,
    "children": children,
    "closed": closed,
    "contains": contains,
    "date": date,
    "descendants": descendants,
    "file": hasfile,
    "follow": follow,
    "grep": grep,
    "head": head,
    "heads": heads,
    "keyword": keyword,
    "limit": limit,
    "max": maxrev,
    "min": minrev,
    "merge": merge,
    "modifies": modifies,
    "id": node,
    "outgoing": outgoing,
    "p1": p1,
    "p2": p2,
    "parents": parents,
    "present": present,
    "removes": removes,
    "reverse": reverse,
    "rev": rev,
    "roots": roots,
    "sort": sort,
    "tag": tag,
    "tagged": tagged,
    "user": user,
}

methods = {
    "range": rangeset,
    "string": stringset,
    "symbol": symbolset,
    "and": andset,
    "or": orset,
    "not": notset,
    "list": listset,
    "func": func,
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
    elif op == 'dagrange':
        return optimize(('and', ('func', ('symbol', 'descendants'), x[1]),
                         ('func', ('symbol', 'ancestors'), x[2])), small)
    elif op == 'dagrangepre':
        return optimize(('func', ('symbol', 'ancestors'), x[1]), small)
    elif op == 'dagrangepost':
        return optimize(('func', ('symbol', 'descendants'), x[1]), small)
    elif op == 'rangepre':
        return optimize(('range', ('string', '0'), x[1]), small)
    elif op == 'rangepost':
        return optimize(('range', x[1], ('string', 'tip')), small)
    elif op == 'negate':
        return optimize(('string',
                         '-' + getstring(x[1], _("can't negate that"))), small)
    elif op in 'string symbol negate':
        return smallbonus, x # single revisions are small
    elif op == 'and' or op == 'dagrange':
        wa, ta = optimize(x[1], True)
        wb, tb = optimize(x[2], True)
        w = min(wa, wb)
        if wa > wb:
            return w, (op, tb, ta)
        return w, (op, ta, tb)
    elif op == 'or':
        wa, ta = optimize(x[1], False)
        wb, tb = optimize(x[2], False)
        if wb < wa:
            wb, wa = wa, wb
        return max(wa, wb), (op, ta, tb)
    elif op == 'not':
        o = optimize(x[1], not small)
        return o[0], (op, o[1])
    elif op == 'group':
        return optimize(x[1], small)
    elif op in 'range list':
        wa, ta = optimize(x[1], small)
        wb, tb = optimize(x[2], small)
        return wa + wb, (op, ta, tb)
    elif op == 'func':
        f = getstring(x[1], _("not a symbol"))
        wa, ta = optimize(x[2], small)
        if f in "grep date user author keyword branch file outgoing":
            w = 10 # slow
        elif f in "modifies adds removes":
            w = 30 # slower
        elif f == "contains":
            w = 100 # very slow
        elif f == "ancestor":
            w = 1 * smallbonus
        elif f in "reverse limit":
            w = 0
        elif f in "sort":
            w = 10 # assume most sorts look at changelog
        else:
            w = 1
        return w + wa, (op, x[1], ta)
    return 1, x

parse = parser.parser(tokenize, elements).parse

def match(spec):
    if not spec:
        raise error.ParseError(_("empty query"))
    tree = parse(spec)
    weight, tree = optimize(tree, True)
    def mfunc(repo, subset):
        return getset(repo, subset, tree)
    return mfunc

def makedoc(topic, doc):
    """Generate and include predicates help in revsets topic."""
    predicates = []
    for name in sorted(symbols):
        text = symbols[name].__doc__
        if not text:
            continue
        text = gettext(text.rstrip())
        lines = text.splitlines()
        lines[1:] = [('  ' + l.strip()) for l in lines[1:]]
        predicates.append('\n'.join(lines))
    predicates = '\n\n'.join(predicates)
    doc = doc.replace('.. predicatesmarker', predicates)
    return doc

# tell hggettext to extract docstrings from these functions:
i18nfunctions = symbols.values()
