# parser.py - simple top-down operator precedence parser for mercurial
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# see http://effbot.org/zone/simple-top-down-parsing.htm and
# http://eli.thegreenplace.net/2010/01/02/top-down-operator-precedence-parsing/
# for background

# takes a tokenizer and elements
# tokenizer is an iterator that returns (type, value, pos) tuples
# elements is a mapping of types to binding strength, primary, prefix, infix
# and suffix actions
# an action is a tree node name, a tree label, and an optional match
# __call__(program) parses program into a labeled tree

from __future__ import absolute_import

from .i18n import _
from . import error

class parser(object):
    def __init__(self, elements, methods=None):
        self._elements = elements
        self._methods = methods
        self.current = None
    def _advance(self):
        'advance the tokenizer'
        t = self.current
        self.current = next(self._iter, None)
        return t
    def _hasnewterm(self):
        'True if next token may start new term'
        return any(self._elements[self.current[0]][1:3])
    def _match(self, m):
        'make sure the tokenizer matches an end condition'
        if self.current[0] != m:
            raise error.ParseError(_("unexpected token: %s") % self.current[0],
                                   self.current[2])
        self._advance()
    def _parseoperand(self, bind, m=None):
        'gather right-hand-side operand until an end condition or binding met'
        if m and self.current[0] == m:
            expr = None
        else:
            expr = self._parse(bind)
        if m:
            self._match(m)
        return expr
    def _parse(self, bind=0):
        token, value, pos = self._advance()
        # handle prefix rules on current token, take as primary if unambiguous
        primary, prefix = self._elements[token][1:3]
        if primary and not (prefix and self._hasnewterm()):
            expr = (primary, value)
        elif prefix:
            expr = (prefix[0], self._parseoperand(*prefix[1:]))
        else:
            raise error.ParseError(_("not a prefix: %s") % token, pos)
        # gather tokens until we meet a lower binding strength
        while bind < self._elements[self.current[0]][0]:
            token, value, pos = self._advance()
            # handle infix rules, take as suffix if unambiguous
            infix, suffix = self._elements[token][3:]
            if suffix and not (infix and self._hasnewterm()):
                expr = (suffix[0], expr)
            elif infix:
                expr = (infix[0], expr, self._parseoperand(*infix[1:]))
            else:
                raise error.ParseError(_("not an infix: %s") % token, pos)
        return expr
    def parse(self, tokeniter):
        'generate a parse tree from tokens'
        self._iter = tokeniter
        self._advance()
        res = self._parse()
        token, value, pos = self.current
        return res, pos
    def eval(self, tree):
        'recursively evaluate a parse tree using node methods'
        if not isinstance(tree, tuple):
            return tree
        return self._methods[tree[0]](*[self.eval(t) for t in tree[1:]])
    def __call__(self, tokeniter):
        'parse tokens into a parse tree and evaluate if methods given'
        t = self.parse(tokeniter)
        if self._methods:
            return self.eval(t)
        return t

def buildargsdict(trees, funcname, keys, keyvaluenode, keynode):
    """Build dict from list containing positional and keyword arguments

    Invalid keywords or too many positional arguments are rejected, but
    missing arguments are just omitted.
    """
    if len(trees) > len(keys):
        raise error.ParseError(_("%(func)s takes at most %(nargs)d arguments")
                               % {'func': funcname, 'nargs': len(keys)})
    args = {}
    # consume positional arguments
    for k, x in zip(keys, trees):
        if x[0] == keyvaluenode:
            break
        args[k] = x
    # remainder should be keyword arguments
    for x in trees[len(args):]:
        if x[0] != keyvaluenode or x[1][0] != keynode:
            raise error.ParseError(_("%(func)s got an invalid argument")
                                   % {'func': funcname})
        k = x[1][1]
        if k not in keys:
            raise error.ParseError(_("%(func)s got an unexpected keyword "
                                     "argument '%(key)s'")
                                   % {'func': funcname, 'key': k})
        if k in args:
            raise error.ParseError(_("%(func)s got multiple values for keyword "
                                     "argument '%(key)s'")
                                   % {'func': funcname, 'key': k})
        args[k] = x[2]
    return args

def unescapestr(s):
    try:
        return s.decode("string_escape")
    except ValueError as e:
        # mangle Python's exception into our format
        raise error.ParseError(str(e).lower())

def _prettyformat(tree, leafnodes, level, lines):
    if not isinstance(tree, tuple) or tree[0] in leafnodes:
        lines.append((level, str(tree)))
    else:
        lines.append((level, '(%s' % tree[0]))
        for s in tree[1:]:
            _prettyformat(s, leafnodes, level + 1, lines)
        lines[-1:] = [(lines[-1][0], lines[-1][1] + ')')]

def prettyformat(tree, leafnodes):
    lines = []
    _prettyformat(tree, leafnodes, 0, lines)
    output = '\n'.join(('  ' * l + s) for l, s in lines)
    return output

def simplifyinfixops(tree, targetnodes):
    """Flatten chained infix operations to reduce usage of Python stack

    >>> def f(tree):
    ...     print prettyformat(simplifyinfixops(tree, ('or',)), ('symbol',))
    >>> f(('or',
    ...     ('or',
    ...       ('symbol', '1'),
    ...       ('symbol', '2')),
    ...     ('symbol', '3')))
    (or
      ('symbol', '1')
      ('symbol', '2')
      ('symbol', '3'))
    >>> f(('func',
    ...     ('symbol', 'p1'),
    ...     ('or',
    ...       ('or',
    ...         ('func',
    ...           ('symbol', 'sort'),
    ...           ('list',
    ...             ('or',
    ...               ('or',
    ...                 ('symbol', '1'),
    ...                 ('symbol', '2')),
    ...               ('symbol', '3')),
    ...             ('negate',
    ...               ('symbol', 'rev')))),
    ...         ('and',
    ...           ('symbol', '4'),
    ...           ('group',
    ...             ('or',
    ...               ('or',
    ...                 ('symbol', '5'),
    ...                 ('symbol', '6')),
    ...               ('symbol', '7'))))),
    ...       ('symbol', '8'))))
    (func
      ('symbol', 'p1')
      (or
        (func
          ('symbol', 'sort')
          (list
            (or
              ('symbol', '1')
              ('symbol', '2')
              ('symbol', '3'))
            (negate
              ('symbol', 'rev'))))
        (and
          ('symbol', '4')
          (group
            (or
              ('symbol', '5')
              ('symbol', '6')
              ('symbol', '7'))))
        ('symbol', '8')))
    """
    if not isinstance(tree, tuple):
        return tree
    op = tree[0]
    if op not in targetnodes:
        return (op,) + tuple(simplifyinfixops(x, targetnodes) for x in tree[1:])

    # walk down left nodes taking each right node. no recursion to left nodes
    # because infix operators are left-associative, i.e. left tree is deep.
    # e.g. '1 + 2 + 3' -> (+ (+ 1 2) 3) -> (+ 1 2 3)
    simplified = []
    x = tree
    while x[0] == op:
        l, r = x[1:]
        simplified.append(simplifyinfixops(r, targetnodes))
        x = l
    simplified.append(simplifyinfixops(x, targetnodes))
    simplified.append(op)
    return tuple(reversed(simplified))

def parseerrordetail(inst):
    """Compose error message from specified ParseError object
    """
    if len(inst.args) > 1:
        return _('at %s: %s') % (inst.args[1], inst.args[0])
    else:
        return inst.args[0]

class alias(object):
    """Parsed result of alias"""

    def __init__(self, name, tree, args, err, replacement):
        self.name = name
        self.tree = tree
        self.args = args
        self.error = err
        self.replacement = replacement
        # whether own `error` information is already shown or not.
        # this avoids showing same warning multiple times at each `findaliases`.
        self.warned = False

class basealiasrules(object):
    """Parsing and expansion rule set of aliases

    This is a helper for fileset/revset/template aliases. A concrete rule set
    should be made by sub-classing this and implementing class/static methods.

    It supports alias expansion of symbol and funciton-call styles::

        # decl = defn
        h = heads(default)
        b($1) = ancestors($1) - ancestors(default)
    """
    # typically a config section, which will be included in error messages
    _section = None
    # tags of symbol and function nodes
    _symbolnode = 'symbol'
    _funcnode = 'func'

    def __new__(cls):
        raise TypeError("'%s' is not instantiatable" % cls.__name__)

    @staticmethod
    def _parse(spec):
        """Parse an alias name, arguments and definition"""
        raise NotImplementedError

    @staticmethod
    def _getlist(tree):
        """Extract a list of arguments from parsed tree"""
        raise NotImplementedError

    @classmethod
    def _builddecl(cls, decl):
        """Parse an alias declaration into ``(name, tree, args, errorstr)``

        This function analyzes the parsed tree. The parsing rule is provided
        by ``_parse()``.

        - ``name``: of declared alias (may be ``decl`` itself at error)
        - ``tree``: parse result (or ``None`` at error)
        - ``args``: list of argument names (or None for symbol declaration)
        - ``errorstr``: detail about detected error (or None)

        >>> sym = lambda x: ('symbol', x)
        >>> symlist = lambda *xs: ('list',) + tuple(sym(x) for x in xs)
        >>> func = lambda n, a: ('func', sym(n), a)
        >>> parsemap = {
        ...     'foo': sym('foo'),
        ...     '$foo': sym('$foo'),
        ...     'foo::bar': ('dagrange', sym('foo'), sym('bar')),
        ...     'foo()': func('foo', None),
        ...     '$foo()': func('$foo', None),
        ...     'foo($1, $2)': func('foo', symlist('$1', '$2')),
        ...     'foo(bar_bar, baz.baz)':
        ...         func('foo', symlist('bar_bar', 'baz.baz')),
        ...     'foo(bar($1, $2))':
        ...         func('foo', func('bar', symlist('$1', '$2'))),
        ...     'foo($1, $2, nested($1, $2))':
        ...         func('foo', (symlist('$1', '$2') +
        ...                      (func('nested', symlist('$1', '$2')),))),
        ...     'foo("bar")': func('foo', ('string', 'bar')),
        ...     'foo($1, $2': error.ParseError('unexpected token: end', 10),
        ...     'foo("bar': error.ParseError('unterminated string', 5),
        ...     'foo($1, $2, $1)': func('foo', symlist('$1', '$2', '$1')),
        ... }
        >>> def parse(expr):
        ...     x = parsemap[expr]
        ...     if isinstance(x, Exception):
        ...         raise x
        ...     return x
        >>> def getlist(tree):
        ...     if not tree:
        ...         return []
        ...     if tree[0] == 'list':
        ...         return list(tree[1:])
        ...     return [tree]
        >>> class aliasrules(basealiasrules):
        ...     _parse = staticmethod(parse)
        ...     _getlist = staticmethod(getlist)
        >>> builddecl = aliasrules._builddecl
        >>> builddecl('foo')
        ('foo', ('symbol', 'foo'), None, None)
        >>> builddecl('$foo')
        ('$foo', None, None, "'$' not for alias arguments")
        >>> builddecl('foo::bar')
        ('foo::bar', None, None, 'invalid format')
        >>> builddecl('foo()')
        ('foo', ('func', ('symbol', 'foo')), [], None)
        >>> builddecl('$foo()')
        ('$foo()', None, None, "'$' not for alias arguments")
        >>> builddecl('foo($1, $2)')
        ('foo', ('func', ('symbol', 'foo')), ['$1', '$2'], None)
        >>> builddecl('foo(bar_bar, baz.baz)')
        ('foo', ('func', ('symbol', 'foo')), ['bar_bar', 'baz.baz'], None)
        >>> builddecl('foo($1, $2, nested($1, $2))')
        ('foo($1, $2, nested($1, $2))', None, None, 'invalid argument list')
        >>> builddecl('foo(bar($1, $2))')
        ('foo(bar($1, $2))', None, None, 'invalid argument list')
        >>> builddecl('foo("bar")')
        ('foo("bar")', None, None, 'invalid argument list')
        >>> builddecl('foo($1, $2')
        ('foo($1, $2', None, None, 'at 10: unexpected token: end')
        >>> builddecl('foo("bar')
        ('foo("bar', None, None, 'at 5: unterminated string')
        >>> builddecl('foo($1, $2, $1)')
        ('foo', None, None, 'argument names collide with each other')
        """
        try:
            tree = cls._parse(decl)
        except error.ParseError as inst:
            return (decl, None, None, parseerrordetail(inst))

        if tree[0] == cls._symbolnode:
            # "name = ...." style
            name = tree[1]
            if name.startswith('$'):
                return (decl, None, None, _("'$' not for alias arguments"))
            return (name, tree, None, None)

        if tree[0] == cls._funcnode and tree[1][0] == cls._symbolnode:
            # "name(arg, ....) = ...." style
            name = tree[1][1]
            if name.startswith('$'):
                return (decl, None, None, _("'$' not for alias arguments"))
            args = []
            for arg in cls._getlist(tree[2]):
                if arg[0] != cls._symbolnode:
                    return (decl, None, None, _("invalid argument list"))
                args.append(arg[1])
            if len(args) != len(set(args)):
                return (name, None, None,
                        _("argument names collide with each other"))
            return (name, tree[:2], args, None)

        return (decl, None, None, _("invalid format"))

    @classmethod
    def _relabelargs(cls, tree, args):
        """Mark alias arguments as ``_aliasarg``"""
        if not isinstance(tree, tuple):
            return tree
        op = tree[0]
        if op != cls._symbolnode:
            return (op,) + tuple(cls._relabelargs(x, args) for x in tree[1:])

        assert len(tree) == 2
        sym = tree[1]
        if sym in args:
            op = '_aliasarg'
        elif sym.startswith('$'):
            raise error.ParseError(_("'$' not for alias arguments"))
        return (op, sym)

    @classmethod
    def _builddefn(cls, defn, args):
        """Parse an alias definition into a tree and marks substitutions

        This function marks alias argument references as ``_aliasarg``. The
        parsing rule is provided by ``_parse()``.

        ``args`` is a list of alias argument names, or None if the alias
        is declared as a symbol.

        >>> parsemap = {
        ...     '$1 or foo': ('or', ('symbol', '$1'), ('symbol', 'foo')),
        ...     '$1 or $bar': ('or', ('symbol', '$1'), ('symbol', '$bar')),
        ...     '$10 or baz': ('or', ('symbol', '$10'), ('symbol', 'baz')),
        ...     '"$1" or "foo"': ('or', ('string', '$1'), ('string', 'foo')),
        ... }
        >>> class aliasrules(basealiasrules):
        ...     _parse = staticmethod(parsemap.__getitem__)
        ...     _getlist = staticmethod(lambda x: [])
        >>> builddefn = aliasrules._builddefn
        >>> def pprint(tree):
        ...     print prettyformat(tree, ('_aliasarg', 'string', 'symbol'))
        >>> args = ['$1', '$2', 'foo']
        >>> pprint(builddefn('$1 or foo', args))
        (or
          ('_aliasarg', '$1')
          ('_aliasarg', 'foo'))
        >>> try:
        ...     builddefn('$1 or $bar', args)
        ... except error.ParseError as inst:
        ...     print parseerrordetail(inst)
        '$' not for alias arguments
        >>> args = ['$1', '$10', 'foo']
        >>> pprint(builddefn('$10 or baz', args))
        (or
          ('_aliasarg', '$10')
          ('symbol', 'baz'))
        >>> pprint(builddefn('"$1" or "foo"', args))
        (or
          ('string', '$1')
          ('string', 'foo'))
        """
        tree = cls._parse(defn)
        if args:
            args = set(args)
        else:
            args = set()
        return cls._relabelargs(tree, args)

    @classmethod
    def build(cls, decl, defn):
        """Parse an alias declaration and definition into an alias object"""
        repl = efmt = None
        name, tree, args, err = cls._builddecl(decl)
        if err:
            efmt = _('failed to parse the declaration of %(section)s '
                     '"%(name)s": %(error)s')
        else:
            try:
                repl = cls._builddefn(defn, args)
            except error.ParseError as inst:
                err = parseerrordetail(inst)
                efmt = _('failed to parse the definition of %(section)s '
                         '"%(name)s": %(error)s')
        if err:
            err = efmt % {'section': cls._section, 'name': name, 'error': err}
        return alias(name, tree, args, err, repl)

    @classmethod
    def buildmap(cls, items):
        """Parse a list of alias (name, replacement) pairs into a dict of
        alias objects"""
        aliases = {}
        for decl, defn in items:
            a = cls.build(decl, defn)
            aliases[a.name] = a
        return aliases
