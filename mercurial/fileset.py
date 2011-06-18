# fileset.py - file set queries for mercurial
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import parser, error, util, merge
from i18n import _

elements = {
    "(": (20, ("group", 1, ")"), ("func", 1, ")")),
    "-": (5, ("negate", 19), ("minus", 5)),
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

globchars = ".*{}[]?/\\"

def tokenize(program):
    pos, l = 0, len(program)
    while pos < l:
        c = program[pos]
        if c.isspace(): # skip inter-token whitespace
            pass
        elif c in "(),-|&+!": # handle simple operators
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
        elif c.isalnum() or c in globchars or ord(c) > 127:
            # gather up a symbol/keyword
            s = pos
            pos += 1
            while pos < l: # find end of symbol
                d = program[pos]
                if not (d.isalnum() or d in globchars or ord(d) > 127):
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

parse = parser.parser(tokenize, elements).parse

def getstring(x, err):
    if x and (x[0] == 'string' or x[0] == 'symbol'):
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

def listset(mctx, a, b):
    raise error.ParseError(_("can't use a list in this context"))

def modified(mctx, x):
    """``modified()``
    File that is modified according to status.
    """
    getargs(x, 0, 0, _("modified takes no arguments"))
    s = mctx.status()[0]
    return [f for f in mctx.subset if f in s]

def added(mctx, x):
    """``added()``
    File that is added according to status.
    """
    getargs(x, 0, 0, _("added takes no arguments"))
    s = mctx.status()[1]
    return [f for f in mctx.subset if f in s]

def removed(mctx, x):
    """``removed()``
    File that is removed according to status.
    """
    getargs(x, 0, 0, _("removed takes no arguments"))
    s = mctx.status()[2]
    return [f for f in mctx.subset if f in s]

def deleted(mctx, x):
    """``deleted()``
    File that is deleted according to status.
    """
    getargs(x, 0, 0, _("deleted takes no arguments"))
    s = mctx.status()[3]
    return [f for f in mctx.subset if f in s]

def unknown(mctx, x):
    """``unknown()``
    File that is unknown according to status. These files will only be
    considered if this predicate is used.
    """
    getargs(x, 0, 0, _("unknown takes no arguments"))
    s = mctx.status()[4]
    return [f for f in mctx.subset if f in s]

def ignored(mctx, x):
    """``ignored()``
    File that is ignored according to status. These files will only be
    considered if this predicate is used.
    """
    getargs(x, 0, 0, _("ignored takes no arguments"))
    s = mctx.status()[5]
    return [f for f in mctx.subset if f in s]

def clean(mctx, x):
    """``clean()``
    File that is clean according to status.
    """
    getargs(x, 0, 0, _("clean takes no arguments"))
    s = mctx.status()[6]
    return [f for f in mctx.subset if f in s]

def func(mctx, a, b):
    if a[0] == 'symbol' and a[1] in symbols:
        return symbols[a[1]](mctx, b)
    raise error.ParseError(_("not a function: %s") % a[1])

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

def binary(mctx, x):
    """``binary()``
    File that appears to be binary (contails NUL bytes).
    """
    getargs(x, 0, 0, _("binary takes no arguments"))
    return [f for f in mctx.subset if util.binary(mctx.ctx[f].data())]

def exec_(mctx, x):
    """``exec()``
    File that is marked as executable.
    """
    getargs(x, 0, 0, _("exec takes no arguments"))
    return [f for f in mctx.subset if mctx.ctx.flags(f) == 'x']

def symlink(mctx, x):
    """``symlink()``
    File that is marked as a symlink.
    """
    getargs(x, 0, 0, _("symlink takes no arguments"))
    return [f for f in mctx.subset if mctx.ctx.flags(f) == 'l']

def resolved(mctx, x):
    """``resolved()``
    File that is marked resolved according to the resolve state.
    """
    getargs(x, 0, 0, _("resolved takes no arguments"))
    if mctx.ctx.rev() is not None:
        return []
    ms = merge.mergestate(mctx.ctx._repo)
    return [f for f in mctx.subset if f in ms and ms[f] == 'r']

def unresolved(mctx, x):
    """``unresolved()``
    File that is marked unresolved according to the resolve state.
    """
    getargs(x, 0, 0, _("unresolved takes no arguments"))
    if mctx.ctx.rev() is not None:
        return []
    ms = merge.mergestate(mctx.ctx._repo)
    return [f for f in mctx.subset if f in ms and ms[f] == 'u']

def hgignore(mctx, x):
    """``resolved()``
    File that matches the active .hgignore pattern.
    """
    getargs(x, 0, 0, _("hgignore takes no arguments"))
    ignore = mctx.ctx._repo.dirstate._ignore
    return [f for f in mctx.subset if ignore(f)]

symbols = {
    'added': added,
    'binary': binary,
    'clean': clean,
    'deleted': deleted,
    'exec': exec_,
    'ignored': ignored,
    'hgignore': hgignore,
    'modified': modified,
    'removed': removed,
    'resolved': resolved,
    'symlink': symlink,
    'unknown': unknown,
    'unresolved': unresolved,
}

methods = {
    'string': stringset,
    'symbol': stringset,
    'and': andset,
    'or': orset,
    'list': listset,
    'group': getset,
    'not': notset,
    'func': func,
}

class matchctx(object):
    def __init__(self, ctx, subset=None, status=None):
        self.ctx = ctx
        self.subset = subset
        self._status = status
    def status(self):
        return self._status
    def matcher(self, patterns):
        return self.ctx.match(patterns)
    def filter(self, files):
        return [f for f in files if f in self.subset]
    def narrow(self, files):
        return matchctx(self.ctx, self.filter(files), self._status)

def _intree(funcs, tree):
    if isinstance(tree, tuple):
        if tree[0] == 'func' and tree[1][0] == 'symbol':
            if tree[1][1] in funcs:
                return True
        for s in tree[1:]:
            if _intree(funcs, s):
                return True
    return False

def getfileset(ctx, expr):
    tree, pos = parse(expr)
    if (pos != len(expr)):
        raise error.ParseError("invalid token", pos)

    # do we need status info?
    if _intree(['modified', 'added', 'removed', 'deleted',
                'unknown', 'ignored', 'clean'], tree):
        unknown = _intree(['unknown'], tree)
        ignored = _intree(['ignored'], tree)

        r = ctx._repo
        status = r.status(ctx.p1(), ctx,
                          unknown=unknown, ignored=ignored, clean=True)
        subset = []
        for c in status:
            subset.extend(c)
    else:
        status = None
        subset = ctx.walk(ctx.match([]))

    return getset(matchctx(ctx, subset, status), tree)

# tell hggettext to extract docstrings from these functions:
i18nfunctions = symbols.values()
