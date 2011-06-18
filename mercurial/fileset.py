# fileset.py - file set queries for mercurial
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import parser, error
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

methods = {
    'string': stringset,
    'symbol': stringset,
    'and': andset,
    'or': orset,
    'list': listset,
    'group': getset,
    'not': notset
}

class matchctx(object):
    def __init__(self, ctx, subset=None):
        self.ctx = ctx
        self.subset = subset
        if subset is None:
            self.subset = ctx.walk(self.matcher([])) # optimize this later
    def matcher(self, patterns):
        return self.ctx.match(patterns)
    def filter(self, files):
        return [f for f in files if f in self.subset]
    def narrow(self, files):
        return matchctx(self.ctx, self.filter(files))

def getfileset(ctx, expr):
    tree, pos = parse(expr)
    if (pos != len(expr)):
        raise error.ParseError("invalid token", pos)
    return getset(matchctx(ctx), tree)
