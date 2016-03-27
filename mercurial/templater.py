# templater.py - template expansion for output
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import re
import types

from .i18n import _
from . import (
    config,
    error,
    minirst,
    parser,
    registrar,
    revset as revsetmod,
    templatefilters,
    templatekw,
    util,
)

# template parsing

elements = {
    # token-type: binding-strength, primary, prefix, infix, suffix
    "(": (20, None, ("group", 1, ")"), ("func", 1, ")"), None),
    ",": (2, None, None, ("list", 2), None),
    "|": (5, None, None, ("|", 5), None),
    "%": (6, None, None, ("%", 6), None),
    ")": (0, None, None, None, None),
    "integer": (0, "integer", None, None, None),
    "symbol": (0, "symbol", None, None, None),
    "string": (0, "string", None, None, None),
    "template": (0, "template", None, None, None),
    "end": (0, None, None, None, None),
}

def tokenize(program, start, end, term=None):
    """Parse a template expression into a stream of tokens, which must end
    with term if specified"""
    pos = start
    while pos < end:
        c = program[pos]
        if c.isspace(): # skip inter-token whitespace
            pass
        elif c in "(,)%|": # handle simple operators
            yield (c, None, pos)
        elif c in '"\'': # handle quoted templates
            s = pos + 1
            data, pos = _parsetemplate(program, s, end, c)
            yield ('template', data, s)
            pos -= 1
        elif c == 'r' and program[pos:pos + 2] in ("r'", 'r"'):
            # handle quoted strings
            c = program[pos + 1]
            s = pos = pos + 2
            while pos < end: # find closing quote
                d = program[pos]
                if d == '\\': # skip over escaped characters
                    pos += 2
                    continue
                if d == c:
                    yield ('string', program[s:pos], s)
                    break
                pos += 1
            else:
                raise error.ParseError(_("unterminated string"), s)
        elif c.isdigit() or c == '-':
            s = pos
            if c == '-': # simply take negate operator as part of integer
                pos += 1
            if pos >= end or not program[pos].isdigit():
                raise error.ParseError(_("integer literal without digits"), s)
            pos += 1
            while pos < end:
                d = program[pos]
                if not d.isdigit():
                    break
                pos += 1
            yield ('integer', program[s:pos], s)
            pos -= 1
        elif (c == '\\' and program[pos:pos + 2] in (r"\'", r'\"')
              or c == 'r' and program[pos:pos + 3] in (r"r\'", r'r\"')):
            # handle escaped quoted strings for compatibility with 2.9.2-3.4,
            # where some of nested templates were preprocessed as strings and
            # then compiled. therefore, \"...\" was allowed. (issue4733)
            #
            # processing flow of _evalifliteral() at 5ab28a2e9962:
            # outer template string    -> stringify()  -> compiletemplate()
            # ------------------------    ------------    ------------------
            # {f("\\\\ {g(\"\\\"\")}"}    \\ {g("\"")}    [r'\\', {g("\"")}]
            #             ~~~~~~~~
            #             escaped quoted string
            if c == 'r':
                pos += 1
                token = 'string'
            else:
                token = 'template'
            quote = program[pos:pos + 2]
            s = pos = pos + 2
            while pos < end: # find closing escaped quote
                if program.startswith('\\\\\\', pos, end):
                    pos += 4 # skip over double escaped characters
                    continue
                if program.startswith(quote, pos, end):
                    # interpret as if it were a part of an outer string
                    data = parser.unescapestr(program[s:pos])
                    if token == 'template':
                        data = _parsetemplate(data, 0, len(data))[0]
                    yield (token, data, s)
                    pos += 1
                    break
                pos += 1
            else:
                raise error.ParseError(_("unterminated string"), s)
        elif c.isalnum() or c in '_':
            s = pos
            pos += 1
            while pos < end: # find end of symbol
                d = program[pos]
                if not (d.isalnum() or d == "_"):
                    break
                pos += 1
            sym = program[s:pos]
            yield ('symbol', sym, s)
            pos -= 1
        elif c == term:
            yield ('end', None, pos + 1)
            return
        else:
            raise error.ParseError(_("syntax error"), pos)
        pos += 1
    if term:
        raise error.ParseError(_("unterminated template expansion"), start)
    yield ('end', None, pos)

def _parsetemplate(tmpl, start, stop, quote=''):
    r"""
    >>> _parsetemplate('foo{bar}"baz', 0, 12)
    ([('string', 'foo'), ('symbol', 'bar'), ('string', '"baz')], 12)
    >>> _parsetemplate('foo{bar}"baz', 0, 12, quote='"')
    ([('string', 'foo'), ('symbol', 'bar')], 9)
    >>> _parsetemplate('foo"{bar}', 0, 9, quote='"')
    ([('string', 'foo')], 4)
    >>> _parsetemplate(r'foo\"bar"baz', 0, 12, quote='"')
    ([('string', 'foo"'), ('string', 'bar')], 9)
    >>> _parsetemplate(r'foo\\"bar', 0, 10, quote='"')
    ([('string', 'foo\\')], 6)
    """
    parsed = []
    sepchars = '{' + quote
    pos = start
    p = parser.parser(elements)
    while pos < stop:
        n = min((tmpl.find(c, pos, stop) for c in sepchars),
                key=lambda n: (n < 0, n))
        if n < 0:
            parsed.append(('string', parser.unescapestr(tmpl[pos:stop])))
            pos = stop
            break
        c = tmpl[n]
        bs = (n - pos) - len(tmpl[pos:n].rstrip('\\'))
        if bs % 2 == 1:
            # escaped (e.g. '\{', '\\\{', but not '\\{')
            parsed.append(('string', parser.unescapestr(tmpl[pos:n - 1]) + c))
            pos = n + 1
            continue
        if n > pos:
            parsed.append(('string', parser.unescapestr(tmpl[pos:n])))
        if c == quote:
            return parsed, n + 1

        parseres, pos = p.parse(tokenize(tmpl, n + 1, stop, '}'))
        parsed.append(parseres)

    if quote:
        raise error.ParseError(_("unterminated string"), start)
    return parsed, pos

def _unnesttemplatelist(tree):
    """Expand list of templates to node tuple

    >>> def f(tree):
    ...     print prettyformat(_unnesttemplatelist(tree))
    >>> f(('template', []))
    ('string', '')
    >>> f(('template', [('string', 'foo')]))
    ('string', 'foo')
    >>> f(('template', [('string', 'foo'), ('symbol', 'rev')]))
    (template
      ('string', 'foo')
      ('symbol', 'rev'))
    >>> f(('template', [('symbol', 'rev')]))  # template(rev) -> str
    (template
      ('symbol', 'rev'))
    >>> f(('template', [('template', [('string', 'foo')])]))
    ('string', 'foo')
    """
    if not isinstance(tree, tuple):
        return tree
    op = tree[0]
    if op != 'template':
        return (op,) + tuple(_unnesttemplatelist(x) for x in tree[1:])

    assert len(tree) == 2
    xs = tuple(_unnesttemplatelist(x) for x in tree[1])
    if not xs:
        return ('string', '')  # empty template ""
    elif len(xs) == 1 and xs[0][0] == 'string':
        return xs[0]  # fast path for string with no template fragment "x"
    else:
        return (op,) + xs

def parse(tmpl):
    """Parse template string into tree"""
    parsed, pos = _parsetemplate(tmpl, 0, len(tmpl))
    assert pos == len(tmpl), 'unquoted template should be consumed'
    return _unnesttemplatelist(('template', parsed))

def _parseexpr(expr):
    """Parse a template expression into tree

    >>> _parseexpr('"foo"')
    ('string', 'foo')
    >>> _parseexpr('foo(bar)')
    ('func', ('symbol', 'foo'), ('symbol', 'bar'))
    >>> _parseexpr('foo(')
    Traceback (most recent call last):
      ...
    ParseError: ('not a prefix: end', 4)
    >>> _parseexpr('"foo" "bar"')
    Traceback (most recent call last):
      ...
    ParseError: ('invalid token', 7)
    """
    p = parser.parser(elements)
    tree, pos = p.parse(tokenize(expr, 0, len(expr)))
    if pos != len(expr):
        raise error.ParseError(_('invalid token'), pos)
    return _unnesttemplatelist(tree)

def prettyformat(tree):
    return parser.prettyformat(tree, ('integer', 'string', 'symbol'))

def compileexp(exp, context, curmethods):
    """Compile parsed template tree to (func, data) pair"""
    t = exp[0]
    if t in curmethods:
        return curmethods[t](exp, context)
    raise error.ParseError(_("unknown method '%s'") % t)

# template evaluation

def getsymbol(exp):
    if exp[0] == 'symbol':
        return exp[1]
    raise error.ParseError(_("expected a symbol, got '%s'") % exp[0])

def getlist(x):
    if not x:
        return []
    if x[0] == 'list':
        return getlist(x[1]) + [x[2]]
    return [x]

def gettemplate(exp, context):
    """Compile given template tree or load named template from map file;
    returns (func, data) pair"""
    if exp[0] in ('template', 'string'):
        return compileexp(exp, context, methods)
    if exp[0] == 'symbol':
        # unlike runsymbol(), here 'symbol' is always taken as template name
        # even if it exists in mapping. this allows us to override mapping
        # by web templates, e.g. 'changelogtag' is redefined in map file.
        return context._load(exp[1])
    raise error.ParseError(_("expected template specifier"))

def evalfuncarg(context, mapping, arg):
    func, data = arg
    # func() may return string, generator of strings or arbitrary object such
    # as date tuple, but filter does not want generator.
    thing = func(context, mapping, data)
    if isinstance(thing, types.GeneratorType):
        thing = stringify(thing)
    return thing

def evalinteger(context, mapping, arg, err):
    v = evalfuncarg(context, mapping, arg)
    try:
        return int(v)
    except (TypeError, ValueError):
        raise error.ParseError(err)

def evalstring(context, mapping, arg):
    func, data = arg
    return stringify(func(context, mapping, data))

def evalstringliteral(context, mapping, arg):
    """Evaluate given argument as string template, but returns symbol name
    if it is unknown"""
    func, data = arg
    if func is runsymbol:
        thing = func(context, mapping, data, default=data)
    else:
        thing = func(context, mapping, data)
    return stringify(thing)

def runinteger(context, mapping, data):
    return int(data)

def runstring(context, mapping, data):
    return data

def _recursivesymbolblocker(key):
    def showrecursion(**args):
        raise error.Abort(_("recursive reference '%s' in template") % key)
    return showrecursion

def _runrecursivesymbol(context, mapping, key):
    raise error.Abort(_("recursive reference '%s' in template") % key)

def runsymbol(context, mapping, key, default=''):
    v = mapping.get(key)
    if v is None:
        v = context._defaults.get(key)
    if v is None:
        # put poison to cut recursion. we can't move this to parsing phase
        # because "x = {x}" is allowed if "x" is a keyword. (issue4758)
        safemapping = mapping.copy()
        safemapping[key] = _recursivesymbolblocker(key)
        try:
            v = context.process(key, safemapping)
        except TemplateNotFound:
            v = default
    if callable(v):
        return v(**mapping)
    return v

def buildtemplate(exp, context):
    ctmpl = [compileexp(e, context, methods) for e in exp[1:]]
    return (runtemplate, ctmpl)

def runtemplate(context, mapping, template):
    for func, data in template:
        yield func(context, mapping, data)

def buildfilter(exp, context):
    arg = compileexp(exp[1], context, methods)
    n = getsymbol(exp[2])
    if n in context._filters:
        filt = context._filters[n]
        return (runfilter, (arg, filt))
    if n in funcs:
        f = funcs[n]
        return (f, [arg])
    raise error.ParseError(_("unknown function '%s'") % n)

def runfilter(context, mapping, data):
    arg, filt = data
    thing = evalfuncarg(context, mapping, arg)
    try:
        return filt(thing)
    except (ValueError, AttributeError, TypeError):
        if isinstance(arg[1], tuple):
            dt = arg[1][1]
        else:
            dt = arg[1]
        raise error.Abort(_("template filter '%s' is not compatible with "
                           "keyword '%s'") % (filt.func_name, dt))

def buildmap(exp, context):
    func, data = compileexp(exp[1], context, methods)
    tfunc, tdata = gettemplate(exp[2], context)
    return (runmap, (func, data, tfunc, tdata))

def runmap(context, mapping, data):
    func, data, tfunc, tdata = data
    d = func(context, mapping, data)
    if util.safehasattr(d, 'itermaps'):
        diter = d.itermaps()
    else:
        try:
            diter = iter(d)
        except TypeError:
            if func is runsymbol:
                raise error.ParseError(_("keyword '%s' is not iterable") % data)
            else:
                raise error.ParseError(_("%r is not iterable") % d)

    for i in diter:
        lm = mapping.copy()
        if isinstance(i, dict):
            lm.update(i)
            lm['originalnode'] = mapping.get('node')
            yield tfunc(context, lm, tdata)
        else:
            # v is not an iterable of dicts, this happen when 'key'
            # has been fully expanded already and format is useless.
            # If so, return the expanded value.
            yield i

def buildfunc(exp, context):
    n = getsymbol(exp[1])
    args = [compileexp(x, context, exprmethods) for x in getlist(exp[2])]
    if n in funcs:
        f = funcs[n]
        return (f, args)
    if n in context._filters:
        if len(args) != 1:
            raise error.ParseError(_("filter %s expects one argument") % n)
        f = context._filters[n]
        return (runfilter, (args[0], f))
    raise error.ParseError(_("unknown function '%s'") % n)

# dict of template built-in functions
funcs = {}

templatefunc = registrar.templatefunc(funcs)

@templatefunc('date(date[, fmt])')
def date(context, mapping, args):
    """Format a date. See :hg:`help dates` for formatting
    strings. The default is a Unix date format, including the timezone:
    "Mon Sep 04 15:13:13 2006 0700"."""
    if not (1 <= len(args) <= 2):
        # i18n: "date" is a keyword
        raise error.ParseError(_("date expects one or two arguments"))

    date = evalfuncarg(context, mapping, args[0])
    fmt = None
    if len(args) == 2:
        fmt = evalstring(context, mapping, args[1])
    try:
        if fmt is None:
            return util.datestr(date)
        else:
            return util.datestr(date, fmt)
    except (TypeError, ValueError):
        # i18n: "date" is a keyword
        raise error.ParseError(_("date expects a date information"))

@templatefunc('diff([includepattern [, excludepattern]])')
def diff(context, mapping, args):
    """Show a diff, optionally
    specifying files to include or exclude."""
    if len(args) > 2:
        # i18n: "diff" is a keyword
        raise error.ParseError(_("diff expects zero, one, or two arguments"))

    def getpatterns(i):
        if i < len(args):
            s = evalstring(context, mapping, args[i]).strip()
            if s:
                return [s]
        return []

    ctx = mapping['ctx']
    chunks = ctx.diff(match=ctx.match([], getpatterns(0), getpatterns(1)))

    return ''.join(chunks)

@templatefunc('fill(text[, width[, initialident[, hangindent]]])')
def fill(context, mapping, args):
    """Fill many
    paragraphs with optional indentation. See the "fill" filter."""
    if not (1 <= len(args) <= 4):
        # i18n: "fill" is a keyword
        raise error.ParseError(_("fill expects one to four arguments"))

    text = evalstring(context, mapping, args[0])
    width = 76
    initindent = ''
    hangindent = ''
    if 2 <= len(args) <= 4:
        width = evalinteger(context, mapping, args[1],
                            # i18n: "fill" is a keyword
                            _("fill expects an integer width"))
        try:
            initindent = evalstring(context, mapping, args[2])
            hangindent = evalstring(context, mapping, args[3])
        except IndexError:
            pass

    return templatefilters.fill(text, width, initindent, hangindent)

@templatefunc('pad(text, width[, fillchar=\' \'[, right=False]])')
def pad(context, mapping, args):
    """Pad text with a
    fill character."""
    if not (2 <= len(args) <= 4):
        # i18n: "pad" is a keyword
        raise error.ParseError(_("pad() expects two to four arguments"))

    width = evalinteger(context, mapping, args[1],
                        # i18n: "pad" is a keyword
                        _("pad() expects an integer width"))

    text = evalstring(context, mapping, args[0])

    right = False
    fillchar = ' '
    if len(args) > 2:
        fillchar = evalstring(context, mapping, args[2])
    if len(args) > 3:
        right = util.parsebool(args[3][1])

    if right:
        return text.rjust(width, fillchar)
    else:
        return text.ljust(width, fillchar)

@templatefunc('indent(text, indentchars[, firstline])')
def indent(context, mapping, args):
    """Indents all non-empty lines
    with the characters given in the indentchars string. An optional
    third parameter will override the indent for the first line only
    if present."""
    if not (2 <= len(args) <= 3):
        # i18n: "indent" is a keyword
        raise error.ParseError(_("indent() expects two or three arguments"))

    text = evalstring(context, mapping, args[0])
    indent = evalstring(context, mapping, args[1])

    if len(args) == 3:
        firstline = evalstring(context, mapping, args[2])
    else:
        firstline = indent

    # the indent function doesn't indent the first line, so we do it here
    return templatefilters.indent(firstline + text, indent)

@templatefunc('get(dict, key)')
def get(context, mapping, args):
    """Get an attribute/key from an object. Some keywords
    are complex types. This function allows you to obtain the value of an
    attribute on these types."""
    if len(args) != 2:
        # i18n: "get" is a keyword
        raise error.ParseError(_("get() expects two arguments"))

    dictarg = evalfuncarg(context, mapping, args[0])
    if not util.safehasattr(dictarg, 'get'):
        # i18n: "get" is a keyword
        raise error.ParseError(_("get() expects a dict as first argument"))

    key = evalfuncarg(context, mapping, args[1])
    return dictarg.get(key)

@templatefunc('if(expr, then[, else])')
def if_(context, mapping, args):
    """Conditionally execute based on the result of
    an expression."""
    if not (2 <= len(args) <= 3):
        # i18n: "if" is a keyword
        raise error.ParseError(_("if expects two or three arguments"))

    test = evalstring(context, mapping, args[0])
    if test:
        yield args[1][0](context, mapping, args[1][1])
    elif len(args) == 3:
        yield args[2][0](context, mapping, args[2][1])

@templatefunc('ifcontains(search, thing, then[, else])')
def ifcontains(context, mapping, args):
    """Conditionally execute based
    on whether the item "search" is in "thing"."""
    if not (3 <= len(args) <= 4):
        # i18n: "ifcontains" is a keyword
        raise error.ParseError(_("ifcontains expects three or four arguments"))

    item = evalstring(context, mapping, args[0])
    items = evalfuncarg(context, mapping, args[1])

    if item in items:
        yield args[2][0](context, mapping, args[2][1])
    elif len(args) == 4:
        yield args[3][0](context, mapping, args[3][1])

@templatefunc('ifeq(expr1, expr2, then[, else])')
def ifeq(context, mapping, args):
    """Conditionally execute based on
    whether 2 items are equivalent."""
    if not (3 <= len(args) <= 4):
        # i18n: "ifeq" is a keyword
        raise error.ParseError(_("ifeq expects three or four arguments"))

    test = evalstring(context, mapping, args[0])
    match = evalstring(context, mapping, args[1])
    if test == match:
        yield args[2][0](context, mapping, args[2][1])
    elif len(args) == 4:
        yield args[3][0](context, mapping, args[3][1])

@templatefunc('join(list, sep)')
def join(context, mapping, args):
    """Join items in a list with a delimiter."""
    if not (1 <= len(args) <= 2):
        # i18n: "join" is a keyword
        raise error.ParseError(_("join expects one or two arguments"))

    joinset = args[0][0](context, mapping, args[0][1])
    if util.safehasattr(joinset, 'itermaps'):
        jf = joinset.joinfmt
        joinset = [jf(x) for x in joinset.itermaps()]

    joiner = " "
    if len(args) > 1:
        joiner = evalstring(context, mapping, args[1])

    first = True
    for x in joinset:
        if first:
            first = False
        else:
            yield joiner
        yield x

@templatefunc('label(label, expr)')
def label(context, mapping, args):
    """Apply a label to generated content. Content with
    a label applied can result in additional post-processing, such as
    automatic colorization."""
    if len(args) != 2:
        # i18n: "label" is a keyword
        raise error.ParseError(_("label expects two arguments"))

    ui = mapping['ui']
    thing = evalstring(context, mapping, args[1])
    # preserve unknown symbol as literal so effects like 'red', 'bold',
    # etc. don't need to be quoted
    label = evalstringliteral(context, mapping, args[0])

    return ui.label(thing, label)

@templatefunc('latesttag([pattern])')
def latesttag(context, mapping, args):
    """The global tags matching the given pattern on the
    most recent globally tagged ancestor of this changeset."""
    if len(args) > 1:
        # i18n: "latesttag" is a keyword
        raise error.ParseError(_("latesttag expects at most one argument"))

    pattern = None
    if len(args) == 1:
        pattern = evalstring(context, mapping, args[0])

    return templatekw.showlatesttags(pattern, **mapping)

@templatefunc('localdate(date[, tz])')
def localdate(context, mapping, args):
    """Converts a date to the specified timezone.
    The default is local date."""
    if not (1 <= len(args) <= 2):
        # i18n: "localdate" is a keyword
        raise error.ParseError(_("localdate expects one or two arguments"))

    date = evalfuncarg(context, mapping, args[0])
    try:
        date = util.parsedate(date)
    except AttributeError:  # not str nor date tuple
        # i18n: "localdate" is a keyword
        raise error.ParseError(_("localdate expects a date information"))
    if len(args) >= 2:
        tzoffset = None
        tz = evalfuncarg(context, mapping, args[1])
        if isinstance(tz, str):
            tzoffset = util.parsetimezone(tz)
        if tzoffset is None:
            try:
                tzoffset = int(tz)
            except (TypeError, ValueError):
                # i18n: "localdate" is a keyword
                raise error.ParseError(_("localdate expects a timezone"))
    else:
        tzoffset = util.makedate()[1]
    return (date[0], tzoffset)

@templatefunc('revset(query[, formatargs...])')
def revset(context, mapping, args):
    """Execute a revision set query. See
    :hg:`help revset`."""
    if not len(args) > 0:
        # i18n: "revset" is a keyword
        raise error.ParseError(_("revset expects one or more arguments"))

    raw = evalstring(context, mapping, args[0])
    ctx = mapping['ctx']
    repo = ctx.repo()

    def query(expr):
        m = revsetmod.match(repo.ui, expr)
        return m(repo)

    if len(args) > 1:
        formatargs = [evalfuncarg(context, mapping, a) for a in args[1:]]
        revs = query(revsetmod.formatspec(raw, *formatargs))
        revs = list(revs)
    else:
        revsetcache = mapping['cache'].setdefault("revsetcache", {})
        if raw in revsetcache:
            revs = revsetcache[raw]
        else:
            revs = query(raw)
            revs = list(revs)
            revsetcache[raw] = revs

    return templatekw.showrevslist("revision", revs, **mapping)

@templatefunc('rstdoc(text, style)')
def rstdoc(context, mapping, args):
    """Format ReStructuredText."""
    if len(args) != 2:
        # i18n: "rstdoc" is a keyword
        raise error.ParseError(_("rstdoc expects two arguments"))

    text = evalstring(context, mapping, args[0])
    style = evalstring(context, mapping, args[1])

    return minirst.format(text, style=style, keep=['verbose'])

@templatefunc('shortest(node, minlength=4)')
def shortest(context, mapping, args):
    """Obtain the shortest representation of
    a node."""
    if not (1 <= len(args) <= 2):
        # i18n: "shortest" is a keyword
        raise error.ParseError(_("shortest() expects one or two arguments"))

    node = evalstring(context, mapping, args[0])

    minlength = 4
    if len(args) > 1:
        minlength = evalinteger(context, mapping, args[1],
                                # i18n: "shortest" is a keyword
                                _("shortest() expects an integer minlength"))

    cl = mapping['ctx']._repo.changelog
    def isvalid(test):
        try:
            try:
                cl.index.partialmatch(test)
            except AttributeError:
                # Pure mercurial doesn't support partialmatch on the index.
                # Fallback to the slow way.
                if cl._partialmatch(test) is None:
                    return False

            try:
                i = int(test)
                # if we are a pure int, then starting with zero will not be
                # confused as a rev; or, obviously, if the int is larger than
                # the value of the tip rev
                if test[0] == '0' or i > len(cl):
                    return True
                return False
            except ValueError:
                return True
        except error.RevlogError:
            return False

    shortest = node
    startlength = max(6, minlength)
    length = startlength
    while True:
        test = node[:length]
        if isvalid(test):
            shortest = test
            if length == minlength or length > startlength:
                return shortest
            length -= 1
        else:
            length += 1
            if len(shortest) <= length:
                return shortest

@templatefunc('strip(text[, chars])')
def strip(context, mapping, args):
    """Strip characters from a string. By default,
    strips all leading and trailing whitespace."""
    if not (1 <= len(args) <= 2):
        # i18n: "strip" is a keyword
        raise error.ParseError(_("strip expects one or two arguments"))

    text = evalstring(context, mapping, args[0])
    if len(args) == 2:
        chars = evalstring(context, mapping, args[1])
        return text.strip(chars)
    return text.strip()

@templatefunc('sub(pattern, replacement, expression)')
def sub(context, mapping, args):
    """Perform text substitution
    using regular expressions."""
    if len(args) != 3:
        # i18n: "sub" is a keyword
        raise error.ParseError(_("sub expects three arguments"))

    pat = evalstring(context, mapping, args[0])
    rpl = evalstring(context, mapping, args[1])
    src = evalstring(context, mapping, args[2])
    try:
        patre = re.compile(pat)
    except re.error:
        # i18n: "sub" is a keyword
        raise error.ParseError(_("sub got an invalid pattern: %s") % pat)
    try:
        yield patre.sub(rpl, src)
    except re.error:
        # i18n: "sub" is a keyword
        raise error.ParseError(_("sub got an invalid replacement: %s") % rpl)

@templatefunc('startswith(pattern, text)')
def startswith(context, mapping, args):
    """Returns the value from the "text" argument
    if it begins with the content from the "pattern" argument."""
    if len(args) != 2:
        # i18n: "startswith" is a keyword
        raise error.ParseError(_("startswith expects two arguments"))

    patn = evalstring(context, mapping, args[0])
    text = evalstring(context, mapping, args[1])
    if text.startswith(patn):
        return text
    return ''

@templatefunc('word(number, text[, separator])')
def word(context, mapping, args):
    """Return the nth word from a string."""
    if not (2 <= len(args) <= 3):
        # i18n: "word" is a keyword
        raise error.ParseError(_("word expects two or three arguments, got %d")
                               % len(args))

    num = evalinteger(context, mapping, args[0],
                      # i18n: "word" is a keyword
                      _("word expects an integer index"))
    text = evalstring(context, mapping, args[1])
    if len(args) == 3:
        splitter = evalstring(context, mapping, args[2])
    else:
        splitter = None

    tokens = text.split(splitter)
    if num >= len(tokens) or num < -len(tokens):
        return ''
    else:
        return tokens[num]

# methods to interpret function arguments or inner expressions (e.g. {_(x)})
exprmethods = {
    "integer": lambda e, c: (runinteger, e[1]),
    "string": lambda e, c: (runstring, e[1]),
    "symbol": lambda e, c: (runsymbol, e[1]),
    "template": buildtemplate,
    "group": lambda e, c: compileexp(e[1], c, exprmethods),
#    ".": buildmember,
    "|": buildfilter,
    "%": buildmap,
    "func": buildfunc,
    }

# methods to interpret top-level template (e.g. {x}, {x|_}, {x % "y"})
methods = exprmethods.copy()
methods["integer"] = exprmethods["symbol"]  # '{1}' as variable

class _aliasrules(parser.basealiasrules):
    """Parsing and expansion rule set of template aliases"""
    _section = _('template alias')
    _parse = staticmethod(_parseexpr)

    @staticmethod
    def _trygetfunc(tree):
        """Return (name, args) if tree is func(...) or ...|filter; otherwise
        None"""
        if tree[0] == 'func' and tree[1][0] == 'symbol':
            return tree[1][1], getlist(tree[2])
        if tree[0] == '|' and tree[2][0] == 'symbol':
            return tree[2][1], [tree[1]]

def expandaliases(tree, aliases):
    """Return new tree of aliases are expanded"""
    aliasmap = _aliasrules.buildmap(aliases)
    return _aliasrules.expand(aliasmap, tree)

# template engine

stringify = templatefilters.stringify

def _flatten(thing):
    '''yield a single stream from a possibly nested set of iterators'''
    if isinstance(thing, str):
        yield thing
    elif not util.safehasattr(thing, '__iter__'):
        if thing is not None:
            yield str(thing)
    else:
        for i in thing:
            if isinstance(i, str):
                yield i
            elif not util.safehasattr(i, '__iter__'):
                if i is not None:
                    yield str(i)
            elif i is not None:
                for j in _flatten(i):
                    yield j

def unquotestring(s):
    '''unwrap quotes if any; otherwise returns unmodified string'''
    if len(s) < 2 or s[0] not in "'\"" or s[0] != s[-1]:
        return s
    return s[1:-1]

class engine(object):
    '''template expansion engine.

    template expansion works like this. a map file contains key=value
    pairs. if value is quoted, it is treated as string. otherwise, it
    is treated as name of template file.

    templater is asked to expand a key in map. it looks up key, and
    looks for strings like this: {foo}. it expands {foo} by looking up
    foo in map, and substituting it. expansion is recursive: it stops
    when there is no more {foo} to replace.

    expansion also allows formatting and filtering.

    format uses key to expand each item in list. syntax is
    {key%format}.

    filter uses function to transform value. syntax is
    {key|filter1|filter2|...}.'''

    def __init__(self, loader, filters=None, defaults=None, aliases=()):
        self._loader = loader
        if filters is None:
            filters = {}
        self._filters = filters
        if defaults is None:
            defaults = {}
        self._defaults = defaults
        self._aliasmap = _aliasrules.buildmap(aliases)
        self._cache = {}  # key: (func, data)

    def _load(self, t):
        '''load, parse, and cache a template'''
        if t not in self._cache:
            # put poison to cut recursion while compiling 't'
            self._cache[t] = (_runrecursivesymbol, t)
            try:
                x = parse(self._loader(t))
                if self._aliasmap:
                    x = _aliasrules.expand(self._aliasmap, x)
                self._cache[t] = compileexp(x, self, methods)
            except: # re-raises
                del self._cache[t]
                raise
        return self._cache[t]

    def process(self, t, mapping):
        '''Perform expansion. t is name of map element to expand.
        mapping contains added elements for use during expansion. Is a
        generator.'''
        func, data = self._load(t)
        return _flatten(func(self, mapping, data))

engines = {'default': engine}

def stylelist():
    paths = templatepaths()
    if not paths:
        return _('no templates found, try `hg debuginstall` for more info')
    dirlist = os.listdir(paths[0])
    stylelist = []
    for file in dirlist:
        split = file.split(".")
        if split[-1] in ('orig', 'rej'):
            continue
        if split[0] == "map-cmdline":
            stylelist.append(split[1])
    return ", ".join(sorted(stylelist))

def _readmapfile(mapfile):
    """Load template elements from the given map file"""
    if not os.path.exists(mapfile):
        raise error.Abort(_("style '%s' not found") % mapfile,
                          hint=_("available styles: %s") % stylelist())

    base = os.path.dirname(mapfile)
    conf = config.config(includepaths=templatepaths())
    conf.read(mapfile)

    cache = {}
    tmap = {}
    for key, val in conf[''].items():
        if not val:
            raise error.ParseError(_('missing value'), conf.source('', key))
        if val[0] in "'\"":
            if val[0] != val[-1]:
                raise error.ParseError(_('unmatched quotes'),
                                       conf.source('', key))
            cache[key] = unquotestring(val)
        else:
            val = 'default', val
            if ':' in val[1]:
                val = val[1].split(':', 1)
            tmap[key] = val[0], os.path.join(base, val[1])
    return cache, tmap

class TemplateNotFound(error.Abort):
    pass

class templater(object):

    def __init__(self, filters=None, defaults=None, cache=None, aliases=(),
                 minchunk=1024, maxchunk=65536):
        '''set up template engine.
        filters is dict of functions. each transforms a value into another.
        defaults is dict of default map definitions.
        aliases is list of alias (name, replacement) pairs.
        '''
        if filters is None:
            filters = {}
        if defaults is None:
            defaults = {}
        if cache is None:
            cache = {}
        self.cache = cache.copy()
        self.map = {}
        self.filters = templatefilters.filters.copy()
        self.filters.update(filters)
        self.defaults = defaults
        self._aliases = aliases
        self.minchunk, self.maxchunk = minchunk, maxchunk
        self.ecache = {}

    @classmethod
    def frommapfile(cls, mapfile, filters=None, defaults=None, cache=None,
                    minchunk=1024, maxchunk=65536):
        """Create templater from the specified map file"""
        t = cls(filters, defaults, cache, [], minchunk, maxchunk)
        cache, tmap = _readmapfile(mapfile)
        t.cache.update(cache)
        t.map = tmap
        return t

    def __contains__(self, key):
        return key in self.cache or key in self.map

    def load(self, t):
        '''Get the template for the given template name. Use a local cache.'''
        if t not in self.cache:
            try:
                self.cache[t] = util.readfile(self.map[t][1])
            except KeyError as inst:
                raise TemplateNotFound(_('"%s" not in template map') %
                                       inst.args[0])
            except IOError as inst:
                raise IOError(inst.args[0], _('template file %s: %s') %
                              (self.map[t][1], inst.args[1]))
        return self.cache[t]

    def __call__(self, t, **mapping):
        ttype = t in self.map and self.map[t][0] or 'default'
        if ttype not in self.ecache:
            try:
                ecls = engines[ttype]
            except KeyError:
                raise error.Abort(_('invalid template engine: %s') % ttype)
            self.ecache[ttype] = ecls(self.load, self.filters, self.defaults,
                                      self._aliases)
        proc = self.ecache[ttype]

        stream = proc.process(t, mapping)
        if self.minchunk:
            stream = util.increasingchunks(stream, min=self.minchunk,
                                           max=self.maxchunk)
        return stream

def templatepaths():
    '''return locations used for template files.'''
    pathsrel = ['templates']
    paths = [os.path.normpath(os.path.join(util.datapath, f))
             for f in pathsrel]
    return [p for p in paths if os.path.isdir(p)]

def templatepath(name):
    '''return location of template file. returns None if not found.'''
    for p in templatepaths():
        f = os.path.join(p, name)
        if os.path.exists(f):
            return f
    return None

def stylemap(styles, paths=None):
    """Return path to mapfile for a given style.

    Searches mapfile in the following locations:
    1. templatepath/style/map
    2. templatepath/map-style
    3. templatepath/map
    """

    if paths is None:
        paths = templatepaths()
    elif isinstance(paths, str):
        paths = [paths]

    if isinstance(styles, str):
        styles = [styles]

    for style in styles:
        # only plain name is allowed to honor template paths
        if (not style
            or style in (os.curdir, os.pardir)
            or os.sep in style
            or os.altsep and os.altsep in style):
            continue
        locations = [os.path.join(style, 'map'), 'map-' + style]
        locations.append('map')

        for path in paths:
            for location in locations:
                mapfile = os.path.join(path, location)
                if os.path.isfile(mapfile):
                    return style, mapfile

    raise RuntimeError("No hgweb templates found in %r" % paths)

def loadfunction(ui, extname, registrarobj):
    """Load template function from specified registrarobj
    """
    for name, func in registrarobj._table.iteritems():
        funcs[name] = func

# tell hggettext to extract docstrings from these functions:
i18nfunctions = funcs.values()
