# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from . import error, parser
from .i18n import _


def eval_script(text, fctx_local, fctx_other):
    """Parse and evaluate text, yielding the name of a merge tool"""
    got = _eval(_parse(text), fctx_local, fctx_other)
    if not isinstance(got, str):
        raise error.EvalError(
            "merge script produced %r instead of 'str'" % type(got).__name__
        )
    return got


def _eval(tree, fctx_local, fctx_other):
    """Evaluate parse tree.

    >>> class fake_context(object):
    ...     def __init__(self, absent):
    ...         self.absent = absent
    ...     def isabsent(self):
    ...         return self.absent

    >>> absent = fake_context(True)
    >>> present = fake_context(False)

    >>> _eval(_parse(":foo"), present, present)
    ':foo'

    >>> _eval(_parse("if(isabsent(local), local_absent, local_not_absent)"), present, absent)
    'local_not_absent'

    >>> _eval(_parse("if(isabsent(local), local_absent, local_not_absent)"), absent, present)
    'local_absent'

    >>> _eval(_parse("isabsent('local')"), present, present)
    Traceback (most recent call last):
      ...
    error.EvalError: 'isabsent' takes a file context (local or other)

    >>> _eval(_parse("if()"), present, present)
    Traceback (most recent call last):
      ...
    EvalError: 'if' takes three arguments, got 0
    """

    if tree[0] == "func":
        func_name = _eval(tree[1], fctx_local, fctx_other)
        if tree[2] is None:
            args = []
        elif tree[2][0] == "list":
            args = tree[2][1:]
        else:
            args = tree[2:]
        if func_name == "if":
            if len(args) != 3:
                raise error.EvalError("'if' takes three arguments, got %d" % len(args))
            if _eval(args[0], fctx_local, fctx_other):
                return _eval(args[1], fctx_local, fctx_other)
            else:
                return _eval(args[2], fctx_local, fctx_other)
        elif func_name == "isabsent":
            if len(args) != 1:
                raise error.EvalError(
                    "'isabsent' takes one argument, got %d" % len(args)
                )
            arg = _eval(args[0], fctx_local, fctx_other)
            if not callable(getattr(arg, "isabsent", None)):
                raise error.EvalError(
                    "'isabsent' takes a file context (local or other)"
                )
            return arg.isabsent()
        else:
            raise error.EvalError("unknown func %s" % func_name)
    elif tree[0] == "symbol":
        if tree[1] == "local":
            return fctx_local
        elif tree[1] == "other":
            return fctx_other
        else:
            return tree[1]
    elif tree[0] == "string":
        return tree[1]
    else:
        raise error.EvalError("unexpected node type %s", tree[0])


_elements = {
    # token-type: binding-strength, primary, prefix, infix, suffix
    "(": (3, None, None, ("func", 1, ")"), None),
    ",": (2, None, None, ("list", 2), None),
    ")": (0, None, None, None, None),
    "string": (0, "string", None, None, None),
    "symbol": (0, "symbol", None, None, None),
    "end": (0, None, None, None, None),
}


def _parse(script):
    """
    >>> _parse(":foo")
    ('symbol', ':foo')

    >>> _parse(" foo ( bar )")
    ('func', ('symbol', 'foo'), ('symbol', 'bar'))

    >>> _parse(" foo ( bar, 'C:\\\\my tool')")
    ('func', ('symbol', 'foo'), ('list', ('symbol', 'bar'), ('string', 'C:\\\\my tool')))

    >>> _parse(" foo ( bar, baz, qux )")
    ('func', ('symbol', 'foo'), ('list', ('symbol', 'bar'), ('symbol', 'baz'), ('symbol', 'qux')))
    """

    p = parser.parser(_elements)
    tree, pos = p.parse(_tokenize(script))
    if pos != len(script):
        raise error.ParseError(_("invalid token"), pos)
    return parser.simplifyinfixops(tree, ("list"))


def _tokenize(text: str):
    """
    >>> list(_tokenize(""))
    [('end', None, 0)]

    >>> list(_tokenize(":foo"))
    [('symbol', ':foo', 0), ('end', None, 4)]

    >>> list(_tokenize("'foo'"))
    [('string', 'foo', 1), ('end', None, 5)]

    >>> list(_tokenize("'\\\\''"))
    [('string', "'", 1), ('end', None, 4)]

    >>> list(_tokenize("'foo( bar \\" baz'"))
    [('string', 'foo( bar " baz', 1), ('end', None, 16)]

    >>> list(_tokenize("'"))
    Traceback (most recent call last):
      ...
    ParseError: ('unterminated string', 0)

    >>> list(_tokenize("'hello\\""))
    Traceback (most recent call last):
      ...
    ParseError: ('unterminated string', 0)

    >>> list(_tokenize(" foo ( bar, baz )"))
    [('symbol', 'foo', 1), ('(', None, 5), ('symbol', 'bar', 7), (',', None, 10), ('symbol', 'baz', 12), (')', None, 16), ('end', None, 17)]
    """
    pos = 0
    while pos < len(text):
        if s := parser.consumestring(text, pos):
            pos = s[0]
            yield s[1]
        elif text[pos].isspace():
            pass
        elif text[pos] in _elements:
            yield (text[pos], None, pos)
        else:
            end = pos + 1
            while end < len(text):
                c = text[end]
                if c.isspace() or c in parser.stringdelimiters or c in _elements:
                    break
                end += 1
            yield ("symbol", text[pos:end], pos)
            pos = end - 1
        pos += 1
    yield ("end", None, pos)
