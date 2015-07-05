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
# elements is a mapping of types to binding strength, prefix, infix and
# suffix actions
# an action is a tree node name, a tree label, and an optional match
# __call__(program) parses program into a labeled tree

import error
from i18n import _

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
        return bool(self._elements[self.current[0]][1])
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
        # handle prefix rules on current token
        prefix = self._elements[token][1]
        if not prefix:
            raise error.ParseError(_("not a prefix: %s") % token, pos)
        if len(prefix) == 1:
            expr = (prefix[0], value)
        else:
            expr = (prefix[0], self._parseoperand(*prefix[1:]))
        # gather tokens until we meet a lower binding strength
        while bind < self._elements[self.current[0]][0]:
            token, value, pos = self._advance()
            infix, suffix = self._elements[token][2:]
            # check for suffix - next token isn't a valid prefix
            if suffix and not self._hasnewterm():
                expr = (suffix[0], expr)
            else:
                # handle infix rules
                if not infix:
                    raise error.ParseError(_("not an infix: %s") % token, pos)
                expr = (infix[0], expr, self._parseoperand(*infix[1:]))
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
