# revsetlang.py - parser, tokenizer and utility for revision set language
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import string

from .i18n import _
from . import (
    error,
    node,
    parser,
    pycompat,
    util,
)

elements = {
    # token-type: binding-strength, primary, prefix, infix, suffix
    "(": (21, None, ("group", 1, ")"), ("func", 1, ")"), None),
    "[": (21, None, None, ("subscript", 1, "]"), None),
    "#": (21, None, None, ("relation", 21), None),
    "##": (20, None, None, ("_concat", 20), None),
    "~": (18, None, None, ("ancestor", 18), None),
    "^": (18, None, None, ("parent", 18), "parentpost"),
    "-": (5, None, ("negate", 19), ("minus", 5), None),
    "::": (17, None, ("dagrangepre", 17), ("dagrange", 17), "dagrangepost"),
    "..": (17, None, ("dagrangepre", 17), ("dagrange", 17), "dagrangepost"),
    ":": (15, "rangeall", ("rangepre", 15), ("range", 15), "rangepost"),
    "not": (10, None, ("not", 10), None, None),
    "!": (10, None, ("not", 10), None, None),
    "and": (5, None, None, ("and", 5), None),
    "&": (5, None, None, ("and", 5), None),
    "%": (5, None, None, ("only", 5), "onlypost"),
    "or": (4, None, None, ("or", 4), None),
    "|": (4, None, None, ("or", 4), None),
    "+": (4, None, None, ("or", 4), None),
    "=": (3, None, None, ("keyvalue", 3), None),
    ",": (2, None, None, ("list", 2), None),
    ")": (0, None, None, None, None),
    "]": (0, None, None, None, None),
    "symbol": (0, "symbol", None, None, None),
    "string": (0, "string", None, None, None),
    "end": (0, None, None, None, None),
}

keywords = {'and', 'or', 'not'}

_quoteletters = {'"', "'"}
_simpleopletters = set(pycompat.iterbytestr("()[]#:=,-|&+!~^%"))

# default set of valid characters for the initial letter of symbols
_syminitletters = set(pycompat.iterbytestr(
    string.ascii_letters.encode('ascii') +
    string.digits.encode('ascii') +
    '._@')) | set(map(pycompat.bytechr, xrange(128, 256)))

# default set of valid characters for non-initial letters of symbols
_symletters = _syminitletters | set(pycompat.iterbytestr('-/'))

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
    program = pycompat.bytestr(program)
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
        elif c in _simpleopletters: # handle simple operators
            yield (c, None, pos)
        elif (c in _quoteletters or c == 'r' and
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

_notset = object()

def getsymbol(x):
    if x and x[0] == 'symbol':
        return x[1]
    raise error.ParseError(_('not a symbol'))

def getstring(x, err):
    if x and (x[0] == 'string' or x[0] == 'symbol'):
        return x[1]
    raise error.ParseError(err)

def getinteger(x, err, default=_notset):
    if not x and default is not _notset:
        return default
    try:
        return int(getstring(x, err))
    except ValueError:
        raise error.ParseError(err)

def getboolean(x, err):
    value = util.parsebool(getsymbol(x))
    if value is not None:
        return value
    raise error.ParseError(err)

def getlist(x):
    if not x:
        return []
    if x[0] == 'list':
        return list(x[1:])
    return [x]

def getrange(x, err):
    if not x:
        raise error.ParseError(err)
    op = x[0]
    if op == 'range':
        return x[1], x[2]
    elif op == 'rangepre':
        return None, x[1]
    elif op == 'rangepost':
        return x[1], None
    elif op == 'rangeall':
        return None, None
    raise error.ParseError(err)

def getargs(x, min, max, err):
    l = getlist(x)
    if len(l) < min or (max >= 0 and len(l) > max):
        raise error.ParseError(err)
    return l

def getargsdict(x, funcname, keys):
    return parser.buildargsdict(getlist(x), funcname, parser.splitargspec(keys),
                                keyvaluenode='keyvalue', keynode='symbol')

def _isnamedfunc(x, funcname):
    """Check if given tree matches named function"""
    return x and x[0] == 'func' and getsymbol(x[1]) == funcname

def _isposargs(x, n):
    """Check if given tree is n-length list of positional arguments"""
    l = getlist(x)
    return len(l) == n and all(y and y[0] != 'keyvalue' for y in l)

def _matchnamedfunc(x, funcname):
    """Return args tree if given tree matches named function; otherwise None

    This can't be used for testing a nullary function since its args tree
    is also None. Use _isnamedfunc() instead.
    """
    if not _isnamedfunc(x, funcname):
        return
    return x[2]

# Constants for ordering requirement, used in _analyze():
#
# If 'define', any nested functions and operations can change the ordering of
# the entries in the set. If 'follow', any nested functions and operations
# should take the ordering specified by the first operand to the '&' operator.
#
# For instance,
#
#   X & (Y | Z)
#   ^   ^^^^^^^
#   |   follow
#   define
#
# will be evaluated as 'or(y(x()), z(x()))', where 'x()' can change the order
# of the entries in the set, but 'y()', 'z()' and 'or()' shouldn't.
#
# 'any' means the order doesn't matter. For instance,
#
#   X & !Y
#        ^
#        any
#
# 'y()' can either enforce its ordering requirement or take the ordering
# specified by 'x()' because 'not()' doesn't care the order.
#
# Transition of ordering requirement:
#
# 1. starts with 'define'
# 2. shifts to 'follow' by 'x & y'
# 3. changes back to 'define' on function call 'f(x)' or function-like
#    operation 'x (f) y' because 'f' may have its own ordering requirement
#    for 'x' and 'y' (e.g. 'first(x)')
#
anyorder = 'any'        # don't care the order
defineorder = 'define'  # should define the order
followorder = 'follow'  # must follow the current order

# transition table for 'x & y', from the current expression 'x' to 'y'
_tofolloworder = {
    anyorder: anyorder,
    defineorder: followorder,
    followorder: followorder,
}

def _matchonly(revs, bases):
    """
    >>> f = lambda *args: _matchonly(*map(parse, args))
    >>> f('ancestors(A)', 'not ancestors(B)')
    ('list', ('symbol', 'A'), ('symbol', 'B'))
    """
    ta = _matchnamedfunc(revs, 'ancestors')
    tb = bases and bases[0] == 'not' and _matchnamedfunc(bases[1], 'ancestors')
    if _isposargs(ta, 1) and _isposargs(tb, 1):
        return ('list', ta, tb)

def _fixops(x):
    """Rewrite raw parsed tree to resolve ambiguous syntax which cannot be
    handled well by our simple top-down parser"""
    if not isinstance(x, tuple):
        return x

    op = x[0]
    if op == 'parent':
        # x^:y means (x^) : y, not x ^ (:y)
        # x^:  means (x^) :,   not x ^ (:)
        post = ('parentpost', x[1])
        if x[2][0] == 'dagrangepre':
            return _fixops(('dagrange', post, x[2][1]))
        elif x[2][0] == 'rangepre':
            return _fixops(('range', post, x[2][1]))
        elif x[2][0] == 'rangeall':
            return _fixops(('rangepost', post))
    elif op == 'or':
        # make number of arguments deterministic:
        # x + y + z -> (or x y z) -> (or (list x y z))
        return (op, _fixops(('list',) + x[1:]))
    elif op == 'subscript' and x[1][0] == 'relation':
        # x#y[z] ternary
        return _fixops(('relsubscript', x[1][1], x[1][2], x[2]))

    return (op,) + tuple(_fixops(y) for y in x[1:])

def _analyze(x, order):
    if x is None:
        return x

    op = x[0]
    if op == 'minus':
        return _analyze(('and', x[1], ('not', x[2])), order)
    elif op == 'only':
        t = ('func', ('symbol', 'only'), ('list', x[1], x[2]))
        return _analyze(t, order)
    elif op == 'onlypost':
        return _analyze(('func', ('symbol', 'only'), x[1]), order)
    elif op == 'dagrangepre':
        return _analyze(('func', ('symbol', 'ancestors'), x[1]), order)
    elif op == 'dagrangepost':
        return _analyze(('func', ('symbol', 'descendants'), x[1]), order)
    elif op == 'negate':
        s = getstring(x[1], _("can't negate that"))
        return _analyze(('string', '-' + s), order)
    elif op in ('string', 'symbol'):
        return x
    elif op == 'and':
        ta = _analyze(x[1], order)
        tb = _analyze(x[2], _tofolloworder[order])
        return (op, ta, tb, order)
    elif op == 'or':
        return (op, _analyze(x[1], order), order)
    elif op == 'not':
        return (op, _analyze(x[1], anyorder), order)
    elif op == 'rangeall':
        return (op, None, order)
    elif op in ('rangepre', 'rangepost', 'parentpost'):
        return (op, _analyze(x[1], defineorder), order)
    elif op == 'group':
        return _analyze(x[1], order)
    elif op in ('dagrange', 'range', 'parent', 'ancestor', 'relation',
                'subscript'):
        ta = _analyze(x[1], defineorder)
        tb = _analyze(x[2], defineorder)
        return (op, ta, tb, order)
    elif op == 'relsubscript':
        ta = _analyze(x[1], defineorder)
        tb = _analyze(x[2], defineorder)
        tc = _analyze(x[3], defineorder)
        return (op, ta, tb, tc, order)
    elif op == 'list':
        return (op,) + tuple(_analyze(y, order) for y in x[1:])
    elif op == 'keyvalue':
        return (op, x[1], _analyze(x[2], order))
    elif op == 'func':
        f = getsymbol(x[1])
        d = defineorder
        if f == 'present':
            # 'present(set)' is known to return the argument set with no
            # modification, so forward the current order to its argument
            d = order
        return (op, x[1], _analyze(x[2], d), order)
    raise ValueError('invalid operator %r' % op)

def analyze(x, order=defineorder):
    """Transform raw parsed tree to evaluatable tree which can be fed to
    optimize() or getset()

    All pseudo operations should be mapped to real operations or functions
    defined in methods or symbols table respectively.

    'order' specifies how the current expression 'x' is ordered (see the
    constants defined above.)
    """
    return _analyze(x, order)

def _optimize(x, small):
    if x is None:
        return 0, x

    smallbonus = 1
    if small:
        smallbonus = .5

    op = x[0]
    if op in ('string', 'symbol'):
        return smallbonus, x # single revisions are small
    elif op == 'and':
        wa, ta = _optimize(x[1], True)
        wb, tb = _optimize(x[2], True)
        order = x[3]
        w = min(wa, wb)

        # (::x and not ::y)/(not ::y and ::x) have a fast path
        tm = _matchonly(ta, tb) or _matchonly(tb, ta)
        if tm:
            return w, ('func', ('symbol', 'only'), tm, order)

        if tb is not None and tb[0] == 'not':
            return wa, ('difference', ta, tb[1], order)

        if wa > wb:
            return w, (op, tb, ta, order)
        return w, (op, ta, tb, order)
    elif op == 'or':
        # fast path for machine-generated expression, that is likely to have
        # lots of trivial revisions: 'a + b + c()' to '_list(a b) + c()'
        order = x[2]
        ws, ts, ss = [], [], []
        def flushss():
            if not ss:
                return
            if len(ss) == 1:
                w, t = ss[0]
            else:
                s = '\0'.join(t[1] for w, t in ss)
                y = ('func', ('symbol', '_list'), ('string', s), order)
                w, t = _optimize(y, False)
            ws.append(w)
            ts.append(t)
            del ss[:]
        for y in getlist(x[1]):
            w, t = _optimize(y, False)
            if t is not None and (t[0] == 'string' or t[0] == 'symbol'):
                ss.append((w, t))
                continue
            flushss()
            ws.append(w)
            ts.append(t)
        flushss()
        if len(ts) == 1:
            return ws[0], ts[0] # 'or' operation is fully optimized out
        if order != defineorder:
            # reorder by weight only when f(a + b) == f(b + a)
            ts = [wt[1] for wt in sorted(zip(ws, ts), key=lambda wt: wt[0])]
        return max(ws), (op, ('list',) + tuple(ts), order)
    elif op == 'not':
        # Optimize not public() to _notpublic() because we have a fast version
        if x[1][:3] == ('func', ('symbol', 'public'), None):
            order = x[1][3]
            newsym = ('func', ('symbol', '_notpublic'), None, order)
            o = _optimize(newsym, not small)
            return o[0], o[1]
        else:
            o = _optimize(x[1], not small)
            order = x[2]
            return o[0], (op, o[1], order)
    elif op == 'rangeall':
        return smallbonus, x
    elif op in ('rangepre', 'rangepost', 'parentpost'):
        o = _optimize(x[1], small)
        order = x[2]
        return o[0], (op, o[1], order)
    elif op in ('dagrange', 'range'):
        wa, ta = _optimize(x[1], small)
        wb, tb = _optimize(x[2], small)
        order = x[3]
        return wa + wb, (op, ta, tb, order)
    elif op in ('parent', 'ancestor', 'relation', 'subscript'):
        w, t = _optimize(x[1], small)
        order = x[3]
        return w, (op, t, x[2], order)
    elif op == 'relsubscript':
        w, t = _optimize(x[1], small)
        order = x[4]
        return w, (op, t, x[2], x[3], order)
    elif op == 'list':
        ws, ts = zip(*(_optimize(y, small) for y in x[1:]))
        return sum(ws), (op,) + ts
    elif op == 'keyvalue':
        w, t = _optimize(x[2], small)
        return w, (op, x[1], t)
    elif op == 'func':
        f = getsymbol(x[1])
        wa, ta = _optimize(x[2], small)
        if f in ('author', 'branch', 'closed', 'date', 'desc', 'file', 'grep',
                 'keyword', 'outgoing', 'user', 'destination'):
            w = 10 # slow
        elif f in ('modifies', 'adds', 'removes'):
            w = 30 # slower
        elif f == "contains":
            w = 100 # very slow
        elif f == "ancestor":
            w = 1 * smallbonus
        elif f in ('reverse', 'limit', 'first', 'wdir', '_intlist'):
            w = 0
        elif f == "sort":
            w = 10 # assume most sorts look at changelog
        else:
            w = 1
        order = x[3]
        return w + wa, (op, x[1], ta, order)
    raise ValueError('invalid operator %r' % op)

def optimize(tree):
    """Optimize evaluatable tree

    All pseudo operations should be transformed beforehand.
    """
    _weight, newtree = _optimize(tree, small=True)
    return newtree

# the set of valid characters for the initial letter of symbols in
# alias declarations and definitions
_aliassyminitletters = _syminitletters | set(pycompat.sysstr('$'))

def _parsewith(spec, lookup=None, syminitletters=None):
    """Generate a parse tree of given spec with given tokenizing options

    >>> _parsewith('foo($1)', syminitletters=_aliassyminitletters)
    ('func', ('symbol', 'foo'), ('symbol', '$1'))
    >>> _parsewith('$1')
    Traceback (most recent call last):
      ...
    ParseError: ("syntax error in revset '$1'", 0)
    >>> _parsewith('foo bar')
    Traceback (most recent call last):
      ...
    ParseError: ('invalid token', 4)
    """
    p = parser.parser(elements)
    tree, pos = p.parse(tokenize(spec, lookup=lookup,
                                 syminitletters=syminitletters))
    if pos != len(spec):
        raise error.ParseError(_('invalid token'), pos)
    return _fixops(parser.simplifyinfixops(tree, ('list', 'or')))

class _aliasrules(parser.basealiasrules):
    """Parsing and expansion rule set of revset aliases"""
    _section = _('revset alias')

    @staticmethod
    def _parse(spec):
        """Parse alias declaration/definition ``spec``

        This allows symbol names to use also ``$`` as an initial letter
        (for backward compatibility), and callers of this function should
        examine whether ``$`` is used also for unexpected symbols or not.
        """
        return _parsewith(spec, syminitletters=_aliassyminitletters)

    @staticmethod
    def _trygetfunc(tree):
        if tree[0] == 'func' and tree[1][0] == 'symbol':
            return tree[1][1], getlist(tree[2])

def expandaliases(tree, aliases, warn=None):
    """Expand aliases in a tree, aliases is a list of (name, value) tuples"""
    aliases = _aliasrules.buildmap(aliases)
    tree = _aliasrules.expand(aliases, tree)
    # warn about problematic (but not referred) aliases
    if warn is not None:
        for name, alias in sorted(aliases.iteritems()):
            if alias.error and not alias.warned:
                warn(_('warning: %s\n') % (alias.error))
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
    return _parsewith(spec, lookup=lookup)

def _quote(s):
    r"""Quote a value in order to make it safe for the revset engine.

    >>> _quote('asdf')
    "'asdf'"
    >>> _quote("asdf'\"")
    '\'asdf\\\'"\''
    >>> _quote('asdf\'')
    "'asdf\\''"
    >>> _quote(1)
    "'1'"
    """
    return "'%s'" % util.escapestr(pycompat.bytestr(s))

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

    def argtype(c, arg):
        if c == 'd':
            return '%d' % int(arg)
        elif c == 's':
            return _quote(arg)
        elif c == 'r':
            parse(arg) # make sure syntax errors are confined
            return '(%s)' % arg
        elif c == 'n':
            return _quote(node.hex(arg))
        elif c == 'b':
            return _quote(arg.branch())

    def listexp(s, t):
        l = len(s)
        if l == 0:
            return "_list('')"
        elif l == 1:
            return argtype(t, s[0])
        elif t == 'd':
            return "_intlist('%s')" % "\0".join('%d' % int(a) for a in s)
        elif t == 's':
            return "_list('%s')" % "\0".join(s)
        elif t == 'n':
            return "_hexlist('%s')" % "\0".join(node.hex(a) for a in s)
        elif t == 'b':
            return "_list('%s')" % "\0".join(a.branch() for a in s)

        m = l // 2
        return '(%s or %s)' % (listexp(s[:m], t), listexp(s[m:], t))

    expr = pycompat.bytestr(expr)
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
                raise error.Abort(_('unexpected revspec format character %s')
                                  % d)
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
