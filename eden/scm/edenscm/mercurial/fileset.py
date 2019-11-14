# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# fileset.py - file set queries for mercurial
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import re

from . import error, merge, parser, pycompat, registrar, scmutil, util
from .i18n import _


elements = {
    # token-type: binding-strength, primary, prefix, infix, suffix
    "(": (20, None, ("group", 1, ")"), ("func", 1, ")"), None),
    "-": (5, None, ("negate", 19), ("minus", 5), None),
    "not": (10, None, ("not", 10), None, None),
    "!": (10, None, ("not", 10), None, None),
    "and": (5, None, None, ("and", 5), None),
    "&": (5, None, None, ("and", 5), None),
    "or": (4, None, None, ("or", 4), None),
    "|": (4, None, None, ("or", 4), None),
    "+": (4, None, None, ("or", 4), None),
    ",": (2, None, None, ("list", 2), None),
    ")": (0, None, None, None, None),
    "symbol": (0, "symbol", None, None, None),
    "string": (0, "string", None, None, None),
    "end": (0, None, None, None, None),
}

keywords = {"and", "or", "not"}

globchars = ".*{}[]?/\\_"


def tokenize(program):
    pos, l = 0, len(program)
    program = pycompat.bytestr(program)
    while pos < l:
        c = program[pos]
        if c.isspace():  # skip inter-token whitespace
            pass
        elif c in "(),-|&+!":  # handle simple operators
            yield (c, None, pos)
        elif (
            c in "\"'" or c == "r" and program[pos : pos + 2] in ("r'", 'r"')
        ):  # handle quoted strings
            if c == "r":
                pos += 1
                c = program[pos]
                decode = lambda x: x
            else:
                decode = parser.unescapestr
            pos += 1
            s = pos
            while pos < l:  # find closing quote
                d = program[pos]
                if d == "\\":  # skip over escaped characters
                    pos += 2
                    continue
                if d == c:
                    yield ("string", decode(program[s:pos]), s)
                    break
                pos += 1
            else:
                raise error.ParseError(_("unterminated string"), s)
        elif c.isalnum() or c in globchars or ord(c) > 127:
            # gather up a symbol/keyword
            s = pos
            pos += 1
            while pos < l:  # find end of symbol
                d = program[pos]
                if not (d.isalnum() or d in globchars or ord(d) > 127):
                    break
                pos += 1
            sym = program[s:pos]
            if sym in keywords:  # operator keywords
                yield (sym, None, s)
            else:
                yield ("symbol", sym, s)
            pos -= 1
        else:
            raise error.ParseError(_("syntax error"), pos)
        pos += 1
    yield ("end", None, pos)


def parse(expr):
    p = parser.parser(elements)
    tree, pos = p.parse(tokenize(expr))
    if pos != len(expr):
        raise error.ParseError(_("invalid token"), pos)
    return tree


def getstring(x, err):
    if x and (x[0] == "string" or x[0] == "symbol"):
        return x[1]
    raise error.ParseError(err)


def getset(mctx, x):
    if not x:
        raise error.ParseError(_("missing argument"))
    return methods[x[0]](mctx, *x[1:])


def stringset(mctx, x):
    m = mctx.matcher([x])
    return [f for f in mctx.subset if m(f)]


def andset(mctx, x, y):
    return getset(mctx.narrow(getset(mctx, x)), y)


def orset(mctx, x, y):
    # needs optimizing
    xl = getset(mctx, x)
    yl = getset(mctx, y)
    return xl + [f for f in yl if f not in xl]


def notset(mctx, x):
    s = set(getset(mctx, x))
    return [r for r in mctx.subset if r not in s]


def minusset(mctx, x, y):
    xl = getset(mctx, x)
    yl = set(getset(mctx, y))
    return [f for f in xl if f not in yl]


def listset(mctx, a, b):
    raise error.ParseError(
        _("can't use a list in this context"), hint=_('see hg help "filesets.x or y"')
    )


# symbols are callable like:
#  fun(mctx, x)
# with:
#  mctx - current matchctx instance
#  x - argument in tree form
symbols = {}

# filesets using matchctx.status()
_statuscallers = set()

# filesets using matchctx.existing()
_existingcallers = set()

predicate = registrar.filesetpredicate()


@predicate("modified()", callstatus=True)
def modified(mctx, x):
    """File that is modified according to :hg:`status`.
    """
    # i18n: "modified" is a keyword
    getargs(x, 0, 0, _("modified takes no arguments"))
    s = set(mctx.status().modified)
    return [f for f in mctx.subset if f in s]


@predicate("added()", callstatus=True)
def added(mctx, x):
    """File that is added according to :hg:`status`.
    """
    # i18n: "added" is a keyword
    getargs(x, 0, 0, _("added takes no arguments"))
    s = set(mctx.status().added)
    return [f for f in mctx.subset if f in s]


@predicate("removed()", callstatus=True)
def removed(mctx, x):
    """File that is removed according to :hg:`status`.
    """
    # i18n: "removed" is a keyword
    getargs(x, 0, 0, _("removed takes no arguments"))
    s = set(mctx.status().removed)
    return [f for f in mctx.subset if f in s]


@predicate("deleted()", callstatus=True)
def deleted(mctx, x):
    """Alias for ``missing()``.
    """
    # i18n: "deleted" is a keyword
    getargs(x, 0, 0, _("deleted takes no arguments"))
    s = set(mctx.status().deleted)
    return [f for f in mctx.subset if f in s]


@predicate("missing()", callstatus=True)
def missing(mctx, x):
    """File that is missing according to :hg:`status`.
    """
    # i18n: "missing" is a keyword
    getargs(x, 0, 0, _("missing takes no arguments"))
    s = set(mctx.status().deleted)
    return [f for f in mctx.subset if f in s]


@predicate("unknown()", callstatus=True)
def unknown(mctx, x):
    """File that is unknown according to :hg:`status`. These files will only be
    considered if this predicate is used.
    """
    # i18n: "unknown" is a keyword
    getargs(x, 0, 0, _("unknown takes no arguments"))
    s = set(mctx.status().unknown)
    return [f for f in mctx.subset if f in s]


@predicate("ignored()", callstatus=True)
def ignored(mctx, x):
    """File that is ignored according to :hg:`status`. These files will only be
    considered if this predicate is used.
    """
    # i18n: "ignored" is a keyword
    getargs(x, 0, 0, _("ignored takes no arguments"))
    s = set(mctx.status().ignored)
    return [f for f in mctx.subset if f in s]


@predicate("clean()", callstatus=True)
def clean(mctx, x):
    """File that is clean according to :hg:`status`.
    """
    # i18n: "clean" is a keyword
    getargs(x, 0, 0, _("clean takes no arguments"))
    s = set(mctx.status().clean)
    return [f for f in mctx.subset if f in s]


def func(mctx, a, b):
    if a[0] == "symbol" and a[1] in symbols:
        funcname = a[1]
        enabled = mctx._existingenabled
        mctx._existingenabled = funcname in _existingcallers
        try:
            return symbols[funcname](mctx, b)
        finally:
            mctx._existingenabled = enabled

    keep = lambda fn: getattr(fn, "__doc__", None) is not None

    syms = [s for (s, fn) in symbols.items() if keep(fn)]
    raise error.UnknownIdentifier(a[1], syms)


def getlist(x):
    if not x:
        return []
    if x[0] == "list":
        return getlist(x[1]) + [x[2]]
    return [x]


def getargs(x, min, max, err):
    l = getlist(x)
    if len(l) < min or len(l) > max:
        raise error.ParseError(err)
    return l


@predicate("binary()", callexisting=True)
def binary(mctx, x):
    """File that appears to be binary (contains NUL bytes).
    """
    # i18n: "binary" is a keyword
    getargs(x, 0, 0, _("binary takes no arguments"))
    return [f for f in mctx.existing() if mctx.ctx[f].isbinary()]


@predicate("exec()", callexisting=True)
def exec_(mctx, x):
    """File that is marked as executable.
    """
    # i18n: "exec" is a keyword
    getargs(x, 0, 0, _("exec takes no arguments"))
    return [f for f in mctx.existing() if mctx.ctx.flags(f) == "x"]


@predicate("symlink()", callexisting=True)
def symlink(mctx, x):
    """File that is marked as a symlink.
    """
    # i18n: "symlink" is a keyword
    getargs(x, 0, 0, _("symlink takes no arguments"))
    return [f for f in mctx.existing() if mctx.ctx.flags(f) == "l"]


@predicate("resolved()")
def resolved(mctx, x):
    """File that is marked resolved according to :hg:`resolve -l`.
    """
    # i18n: "resolved" is a keyword
    getargs(x, 0, 0, _("resolved takes no arguments"))
    if mctx.ctx.rev() is not None:
        return []
    ms = merge.mergestate.read(mctx.ctx.repo())
    return [f for f in mctx.subset if f in ms and ms[f] == "r"]


@predicate("unresolved()")
def unresolved(mctx, x):
    """File that is marked unresolved according to :hg:`resolve -l`.
    """
    # i18n: "unresolved" is a keyword
    getargs(x, 0, 0, _("unresolved takes no arguments"))
    if mctx.ctx.rev() is not None:
        return []
    ms = merge.mergestate.read(mctx.ctx.repo())
    return [f for f in mctx.subset if f in ms and ms[f] == "u"]


@predicate("hgignore()")
def hgignore(mctx, x):
    """File that matches the active .hgignore pattern. (DEPRECATED)
    """
    # i18n: "hgignore" is a keyword
    getargs(x, 0, 0, _("hgignore takes no arguments"))
    repo = mctx.ctx.repo()
    ui = repo.ui
    if ui.configbool("experimental", "disallowhgignorefileset"):
        raise error.Abort(_("hgignore() is deprecated. Use gitignore() instead."))
    ignore = repo.dirstate._ignore
    return [f for f in mctx.subset if ignore(f)]


@predicate("gitignore()")
def gitignore(mctx, x):
    """File that matches the active .gitignore pattern.
    """
    # i18n: "gitignore" is a keyword
    getargs(x, 0, 0, _("gitignore takes no arguments"))
    repo = mctx.ctx.repo()
    ignore = repo.dirstate._ignore
    return [f for f in mctx.subset if ignore(f)]


@predicate("portable()")
def portable(mctx, x):
    """File that has a portable name. (This doesn't include filenames with case
    collisions.)
    """
    # i18n: "portable" is a keyword
    getargs(x, 0, 0, _("portable takes no arguments"))
    checkwinfilename = util.checkwinfilename
    return [f for f in mctx.subset if checkwinfilename(f) is None]


@predicate("grep(regex)", callexisting=True)
def grep(mctx, x):
    """File contains the given regular expression.
    """
    try:
        # i18n: "grep" is a keyword
        r = re.compile(getstring(x, _("grep requires a pattern")))
    except re.error as e:
        raise error.ParseError(_("invalid match pattern: %s") % e)
    return [f for f in mctx.existing() if r.search(mctx.ctx[f].data())]


def _sizetomax(s):
    try:
        s = s.strip().lower()
        # Iterate in reverse order so we check "b" last, otherwise we'd match
        # "b" instead of "kb".
        for k, v in reversed(util._sizeunits):
            if s.endswith(k):
                # max(4k) = 5k - 1, max(4.5k) = 4.6k - 1
                n = s[: -len(k)]
                inc = 1.0
                if "." in n:
                    inc /= 10 ** len(n.split(".")[1])
                return int((float(n) + inc) * v) - 1
        # no extension, this is a precise value
        return int(s)
    except ValueError:
        raise error.ParseError(_("couldn't parse size: %s") % s)


@predicate("size(expression)", callexisting=True)
def size(mctx, x):
    """File size matches the given expression. Examples:

    - size('1k') - files from 1024 to 2047 bytes
    - size('< 20k') - files less than 20480 bytes
    - size('>= .5MB') - files at least 524288 bytes
    - size('4k - 1MB') - files from 4096 bytes to 1048576 bytes
    """

    # i18n: "size" is a keyword
    expr = getstring(x, _("size requires an expression")).strip()
    if "-" in expr:  # do we have a range?
        a, b = expr.split("-", 1)
        a = util.sizetoint(a)
        b = util.sizetoint(b)
        m = lambda x: x >= a and x <= b
    elif expr.startswith("<="):
        a = util.sizetoint(expr[2:])
        m = lambda x: x <= a
    elif expr.startswith("<"):
        a = util.sizetoint(expr[1:])
        m = lambda x: x < a
    elif expr.startswith(">="):
        a = util.sizetoint(expr[2:])
        m = lambda x: x >= a
    elif expr.startswith(">"):
        a = util.sizetoint(expr[1:])
        m = lambda x: x > a
    elif expr[0].isdigit or expr[0] == ".":
        a = util.sizetoint(expr)
        b = _sizetomax(expr)
        m = lambda x: x >= a and x <= b
    else:
        raise error.ParseError(_("couldn't parse size: %s") % expr)

    return [f for f in mctx.existing() if m(mctx.ctx[f].size())]


@predicate("encoding(name)", callexisting=True)
def encoding(mctx, x):
    """File can be successfully decoded with the given character
    encoding. May not be useful for encodings other than ASCII and
    UTF-8.
    """

    # i18n: "encoding" is a keyword
    enc = getstring(x, _("encoding requires an encoding name"))

    s = []
    for f in mctx.existing():
        d = mctx.ctx[f].data()
        try:
            d.decode(enc)
        except LookupError:
            raise error.Abort(_("unknown encoding '%s'") % enc)
        except UnicodeDecodeError:
            continue
        s.append(f)

    return s


@predicate("eol(style)", callexisting=True)
def eol(mctx, x):
    """File contains newlines of the given style (dos, unix, mac). Binary
    files are excluded, files with mixed line endings match multiple
    styles.
    """

    # i18n: "eol" is a keyword
    enc = getstring(x, _("eol requires a style name"))

    s = []
    for f in mctx.existing():
        d = mctx.ctx[f].data()
        if util.binary(d):
            continue
        if (enc == "dos" or enc == "win") and "\r\n" in d:
            s.append(f)
        elif enc == "unix" and re.search("(?<!\r)\n", d):
            s.append(f)
        elif enc == "mac" and re.search("\r(?!\n)", d):
            s.append(f)
    return s


@predicate("copied()")
def copied(mctx, x):
    """File that is recorded as being copied.
    """
    # i18n: "copied" is a keyword
    getargs(x, 0, 0, _("copied takes no arguments"))
    s = []
    for f in mctx.subset:
        p = mctx.ctx[f].parents()
        if p and p[0].path() != f:
            s.append(f)
    return s


@predicate("revs(revs, pattern)")
def revs(mctx, x):
    """Evaluate set in the specified revisions. If the revset match multiple
    revs, this will return file matching pattern in any of the revision.
    """
    # i18n: "revs" is a keyword
    r, x = getargs(x, 2, 2, _("revs takes two arguments"))
    # i18n: "revs" is a keyword
    revspec = getstring(r, _("first argument to revs must be a revision"))
    repo = mctx.ctx.repo()
    revs = scmutil.revrange(repo, [revspec])

    found = set()
    result = []
    for r in revs:
        ctx = repo[r]
        for f in getset(mctx.switch(ctx, _buildstatus(ctx, x)), x):
            if f not in found:
                found.add(f)
                result.append(f)
    return result


@predicate("status(base, rev, pattern)")
def status(mctx, x):
    """Evaluate predicate using status change between ``base`` and
    ``rev``. Examples:

    - ``status(3, 7, added())`` - matches files added from "3" to "7"
    """
    repo = mctx.ctx.repo()
    # i18n: "status" is a keyword
    b, r, x = getargs(x, 3, 3, _("status takes three arguments"))
    # i18n: "status" is a keyword
    baseerr = _("first argument to status must be a revision")
    baserevspec = getstring(b, baseerr)
    if not baserevspec:
        raise error.ParseError(baseerr)
    reverr = _("second argument to status must be a revision")
    revspec = getstring(r, reverr)
    if not revspec:
        raise error.ParseError(reverr)
    basenode, node = scmutil.revpair(repo, [baserevspec, revspec])
    basectx = repo[basenode]
    ctx = repo[node]
    return getset(mctx.switch(ctx, _buildstatus(ctx, x, basectx=basectx)), x)


methods = {
    "string": stringset,
    "symbol": stringset,
    "and": andset,
    "or": orset,
    "minus": minusset,
    "list": listset,
    "group": getset,
    "not": notset,
    "func": func,
}


class matchctx(object):
    def __init__(self, ctx, subset, status=None):
        self.ctx = ctx
        self.subset = subset
        self._status = status
        self._existingenabled = False

    def status(self):
        return self._status

    def matcher(self, patterns):
        return self.ctx.match(patterns)

    def filter(self, files):
        return [f for f in files if f in self.subset]

    def existing(self):
        assert self._existingenabled, "unexpected existing() invocation"
        if self._status is not None:
            removed = set(self._status[3])
            unknown = set(self._status[4] + self._status[5])
        else:
            removed = set()
            unknown = set()
        return (
            f
            for f in self.subset
            if (f in self.ctx and f not in removed) or f in unknown
        )

    def narrow(self, files):
        return matchctx(self.ctx, self.filter(files), self._status)

    def switch(self, ctx, status=None):
        subset = self.filter(_buildsubset(ctx, status))
        return matchctx(ctx, subset, status)


class fullmatchctx(matchctx):
    """A match context where any files in any revisions should be valid"""

    def __init__(self, ctx, status=None):
        subset = _buildsubset(ctx, status)
        super(fullmatchctx, self).__init__(ctx, subset, status)

    def switch(self, ctx, status=None):
        return fullmatchctx(ctx, status)

    def narrow(self, files):
        return matchctx(self.ctx, files, self._status)


# filesets using matchctx.switch()
_switchcallers = ["revs", "status"]


def _intree(funcs, tree):
    if isinstance(tree, tuple):
        if tree[0] == "func" and tree[1][0] == "symbol":
            if tree[1][1] in funcs:
                return True
            if tree[1][1] in _switchcallers:
                # arguments won't be evaluated in the current context
                return False
        for s in tree[1:]:
            if _intree(funcs, s):
                return True
    return False


def _buildsubset(ctx, status):
    if status:
        subset = []
        for c in status:
            subset.extend(c)
        return subset
    else:
        return list(ctx.walk(ctx.match([])))


def getfileset(ctx, expr):
    tree = parse(expr)
    return getset(fullmatchctx(ctx, _buildstatus(ctx, tree)), tree)


def _buildstatus(ctx, tree, basectx=None):
    # do we need status info?

    # temporaty boolean to simplify the next conditional
    purewdir = ctx.rev() is None and basectx is None

    if (
        _intree(_statuscallers, tree)
        or
        # Using matchctx.existing() on a workingctx requires us to check
        # for deleted files.
        (purewdir and _intree(_existingcallers, tree))
    ):
        unknown = _intree(["unknown"], tree)
        ignored = _intree(["ignored"], tree)

        r = ctx.repo()
        if basectx is None:
            basectx = ctx.p1()
        return r.status(basectx, ctx, unknown=unknown, ignored=ignored, clean=True)
    else:
        return None


def prettyformat(tree):
    return parser.prettyformat(tree, ("string", "symbol"))


def loadpredicate(ui, extname, registrarobj):
    """Load fileset predicates from specified registrarobj
    """
    for name, func in registrarobj._table.iteritems():
        symbols[name] = func
        if func._callstatus:
            _statuscallers.add(name)
        if func._callexisting:
            _existingcallers.add(name)


# load built-in predicates explicitly to setup _statuscallers/_existingcallers
loadpredicate(None, None, predicate)

# tell hggettext to extract docstrings from these functions:
i18nfunctions = symbols.values()
