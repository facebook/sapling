# templater.py - template expansion for output
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import os, re
import util, config, templatefilters, templatekw, parser, error
import revset as revsetmod
import types
import minirst

# template parsing

elements = {
    "(": (20, ("group", 1, ")"), ("func", 1, ")")),
    ",": (2, None, ("list", 2)),
    "|": (5, None, ("|", 5)),
    "%": (6, None, ("%", 6)),
    ")": (0, None, None),
    "integer": (0, ("integer",), None),
    "symbol": (0, ("symbol",), None),
    "string": (0, ("string",), None),
    "rawstring": (0, ("rawstring",), None),
    "end": (0, None, None),
}

def tokenizer(data):
    program, start, end = data
    pos = start
    while pos < end:
        c = program[pos]
        if c.isspace(): # skip inter-token whitespace
            pass
        elif c in "(,)%|": # handle simple operators
            yield (c, None, pos)
        elif (c in '"\'' or c == 'r' and
              program[pos:pos + 2] in ("r'", 'r"')): # handle quoted strings
            if c == 'r':
                pos += 1
                c = program[pos]
                decode = False
            else:
                decode = True
            pos += 1
            s = pos
            while pos < end: # find closing quote
                d = program[pos]
                if decode and d == '\\': # skip over escaped characters
                    pos += 2
                    continue
                if d == c:
                    if not decode:
                        yield ('rawstring', program[s:pos], s)
                        break
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
        elif c == '}':
            pos += 1
            break
        else:
            raise error.ParseError(_("syntax error"), pos)
        pos += 1
    yield ('end', None, pos)

def compiletemplate(tmpl, context, strtoken="string"):
    parsed = []
    pos, stop = 0, len(tmpl)
    p = parser.parser(tokenizer, elements)
    while pos < stop:
        n = tmpl.find('{', pos)
        if n < 0:
            parsed.append((strtoken, tmpl[pos:]))
            break
        bs = (n - pos) - len(tmpl[pos:n].rstrip('\\'))
        if strtoken == 'string' and bs % 2 == 1:
            # escaped (e.g. '\{', '\\\{', but not '\\{' nor r'\{')
            parsed.append((strtoken, (tmpl[pos:n - 1] + "{")))
            pos = n + 1
            continue
        if n > pos:
            parsed.append((strtoken, tmpl[pos:n]))

        pd = [tmpl, n + 1, stop]
        parseres, pos = p.parse(pd)
        parsed.append(parseres)

    return [compileexp(e, context, methods) for e in parsed]

def compileexp(exp, context, curmethods):
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

def getfilter(exp, context):
    f = getsymbol(exp)
    if f not in context._filters:
        raise error.ParseError(_("unknown function '%s'") % f)
    return context._filters[f]

def gettemplate(exp, context):
    if exp[0] == 'string' or exp[0] == 'rawstring':
        return compiletemplate(exp[1], context, strtoken=exp[0])
    if exp[0] == 'symbol':
        return context._load(exp[1])
    raise error.ParseError(_("expected template specifier"))

def runinteger(context, mapping, data):
    return int(data)

def runstring(context, mapping, data):
    return data.decode("string-escape")

def runrawstring(context, mapping, data):
    return data

def runsymbol(context, mapping, key):
    v = mapping.get(key)
    if v is None:
        v = context._defaults.get(key)
    if v is None:
        try:
            v = context.process(key, mapping)
        except TemplateNotFound:
            v = ''
    if callable(v):
        return v(**mapping)
    if isinstance(v, types.GeneratorType):
        v = list(v)
    return v

def buildfilter(exp, context):
    func, data = compileexp(exp[1], context, methods)
    filt = getfilter(exp[2], context)
    return (runfilter, (func, data, filt))

def runfilter(context, mapping, data):
    func, data, filt = data
    # func() may return string, generator of strings or arbitrary object such
    # as date tuple, but filter does not want generator.
    thing = func(context, mapping, data)
    if isinstance(thing, types.GeneratorType):
        thing = stringify(thing)
    try:
        return filt(thing)
    except (ValueError, AttributeError, TypeError):
        if isinstance(data, tuple):
            dt = data[1]
        else:
            dt = data
        raise util.Abort(_("template filter '%s' is not compatible with "
                           "keyword '%s'") % (filt.func_name, dt))

def buildmap(exp, context):
    func, data = compileexp(exp[1], context, methods)
    ctmpl = gettemplate(exp[2], context)
    return (runmap, (func, data, ctmpl))

def runtemplate(context, mapping, template):
    for func, data in template:
        yield func(context, mapping, data)

def runmap(context, mapping, data):
    func, data, ctmpl = data
    d = func(context, mapping, data)
    if callable(d):
        d = d()

    lm = mapping.copy()

    for i in d:
        if isinstance(i, dict):
            lm.update(i)
            lm['originalnode'] = mapping.get('node')
            yield runtemplate(context, lm, ctmpl)
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
        return (runfilter, (args[0][0], args[0][1], f))
    raise error.ParseError(_("unknown function '%s'") % n)

def date(context, mapping, args):
    """:date(date[, fmt]): Format a date. See :hg:`help dates` for formatting
    strings."""
    if not (1 <= len(args) <= 2):
        # i18n: "date" is a keyword
        raise error.ParseError(_("date expects one or two arguments"))

    date = args[0][0](context, mapping, args[0][1])
    fmt = None
    if len(args) == 2:
        fmt = stringify(args[1][0](context, mapping, args[1][1]))
    try:
        if fmt is None:
            return util.datestr(date)
        else:
            return util.datestr(date, fmt)
    except (TypeError, ValueError):
        # i18n: "date" is a keyword
        raise error.ParseError(_("date expects a date information"))

def diff(context, mapping, args):
    """:diff([includepattern [, excludepattern]]): Show a diff, optionally
    specifying files to include or exclude."""
    if len(args) > 2:
        # i18n: "diff" is a keyword
        raise error.ParseError(_("diff expects one, two or no arguments"))

    def getpatterns(i):
        if i < len(args):
            s = args[i][1].strip()
            if s:
                return [s]
        return []

    ctx = mapping['ctx']
    chunks = ctx.diff(match=ctx.match([], getpatterns(0), getpatterns(1)))

    return ''.join(chunks)

def fill(context, mapping, args):
    """:fill(text[, width[, initialident[, hangindent]]]): Fill many
    paragraphs with optional indentation. See the "fill" filter."""
    if not (1 <= len(args) <= 4):
        # i18n: "fill" is a keyword
        raise error.ParseError(_("fill expects one to four arguments"))

    text = stringify(args[0][0](context, mapping, args[0][1]))
    width = 76
    initindent = ''
    hangindent = ''
    if 2 <= len(args) <= 4:
        try:
            width = int(stringify(args[1][0](context, mapping, args[1][1])))
        except ValueError:
            # i18n: "fill" is a keyword
            raise error.ParseError(_("fill expects an integer width"))
        try:
            initindent = stringify(_evalifliteral(args[2], context, mapping))
            hangindent = stringify(_evalifliteral(args[3], context, mapping))
        except IndexError:
            pass

    return templatefilters.fill(text, width, initindent, hangindent)

def pad(context, mapping, args):
    """:pad(text, width[, fillchar=' '[, right=False]]): Pad text with a
    fill character."""
    if not (2 <= len(args) <= 4):
        # i18n: "pad" is a keyword
        raise error.ParseError(_("pad() expects two to four arguments"))

    width = int(args[1][1])

    text = stringify(args[0][0](context, mapping, args[0][1]))
    if args[0][0] == runstring:
        text = stringify(runtemplate(context, mapping,
            compiletemplate(text, context)))

    right = False
    fillchar = ' '
    if len(args) > 2:
        fillchar = stringify(args[2][0](context, mapping, args[2][1]))
    if len(args) > 3:
        right = util.parsebool(args[3][1])

    if right:
        return text.rjust(width, fillchar)
    else:
        return text.ljust(width, fillchar)

def indent(context, mapping, args):
    """:indent(text, indentchars[, firstline]): Indents all non-empty lines
    with the characters given in the indentchars string. An optional
    third parameter will override the indent for the first line only
    if present."""
    if not (2 <= len(args) <= 3):
        # i18n: "indent" is a keyword
        raise error.ParseError(_("indent() expects two or three arguments"))

    text = stringify(args[0][0](context, mapping, args[0][1]))
    indent = stringify(args[1][0](context, mapping, args[1][1]))

    if len(args) == 3:
        firstline = stringify(args[2][0](context, mapping, args[2][1]))
    else:
        firstline = indent

    # the indent function doesn't indent the first line, so we do it here
    return templatefilters.indent(firstline + text, indent)

def get(context, mapping, args):
    """:get(dict, key): Get an attribute/key from an object. Some keywords
    are complex types. This function allows you to obtain the value of an
    attribute on these type."""
    if len(args) != 2:
        # i18n: "get" is a keyword
        raise error.ParseError(_("get() expects two arguments"))

    dictarg = args[0][0](context, mapping, args[0][1])
    if not util.safehasattr(dictarg, 'get'):
        # i18n: "get" is a keyword
        raise error.ParseError(_("get() expects a dict as first argument"))

    key = args[1][0](context, mapping, args[1][1])
    yield dictarg.get(key)

def _evalifliteral(arg, context, mapping):
    t = stringify(arg[0](context, mapping, arg[1]))
    if arg[0] == runstring or arg[0] == runrawstring:
        yield runtemplate(context, mapping,
                          compiletemplate(t, context, strtoken='rawstring'))
    else:
        yield t

def if_(context, mapping, args):
    """:if(expr, then[, else]): Conditionally execute based on the result of
    an expression."""
    if not (2 <= len(args) <= 3):
        # i18n: "if" is a keyword
        raise error.ParseError(_("if expects two or three arguments"))

    test = stringify(args[0][0](context, mapping, args[0][1]))
    if test:
        yield _evalifliteral(args[1], context, mapping)
    elif len(args) == 3:
        yield _evalifliteral(args[2], context, mapping)

def ifcontains(context, mapping, args):
    """:ifcontains(search, thing, then[, else]): Conditionally execute based
    on whether the item "search" is in "thing"."""
    if not (3 <= len(args) <= 4):
        # i18n: "ifcontains" is a keyword
        raise error.ParseError(_("ifcontains expects three or four arguments"))

    item = stringify(args[0][0](context, mapping, args[0][1]))
    items = args[1][0](context, mapping, args[1][1])

    if item in items:
        yield _evalifliteral(args[2], context, mapping)
    elif len(args) == 4:
        yield _evalifliteral(args[3], context, mapping)

def ifeq(context, mapping, args):
    """:ifeq(expr1, expr2, then[, else]): Conditionally execute based on
    whether 2 items are equivalent."""
    if not (3 <= len(args) <= 4):
        # i18n: "ifeq" is a keyword
        raise error.ParseError(_("ifeq expects three or four arguments"))

    test = stringify(args[0][0](context, mapping, args[0][1]))
    match = stringify(args[1][0](context, mapping, args[1][1]))
    if test == match:
        yield _evalifliteral(args[2], context, mapping)
    elif len(args) == 4:
        yield _evalifliteral(args[3], context, mapping)

def join(context, mapping, args):
    """:join(list, sep): Join items in a list with a delimiter."""
    if not (1 <= len(args) <= 2):
        # i18n: "join" is a keyword
        raise error.ParseError(_("join expects one or two arguments"))

    joinset = args[0][0](context, mapping, args[0][1])
    if callable(joinset):
        jf = joinset.joinfmt
        joinset = [jf(x) for x in joinset()]

    joiner = " "
    if len(args) > 1:
        joiner = stringify(args[1][0](context, mapping, args[1][1]))

    first = True
    for x in joinset:
        if first:
            first = False
        else:
            yield joiner
        yield x

def label(context, mapping, args):
    """:label(label, expr): Apply a label to generated content. Content with
    a label applied can result in additional post-processing, such as
    automatic colorization."""
    if len(args) != 2:
        # i18n: "label" is a keyword
        raise error.ParseError(_("label expects two arguments"))

    # ignore args[0] (the label string) since this is supposed to be a a no-op
    yield _evalifliteral(args[1], context, mapping)

def revset(context, mapping, args):
    """:revset(query[, formatargs...]): Execute a revision set query. See
    :hg:`help revset`."""
    if not len(args) > 0:
        # i18n: "revset" is a keyword
        raise error.ParseError(_("revset expects one or more arguments"))

    raw = args[0][1]
    ctx = mapping['ctx']
    repo = ctx.repo()

    def query(expr):
        m = revsetmod.match(repo.ui, expr)
        return m(repo)

    if len(args) > 1:
        formatargs = list([a[0](context, mapping, a[1]) for a in args[1:]])
        revs = query(revsetmod.formatspec(raw, *formatargs))
        revs = list([str(r) for r in revs])
    else:
        revsetcache = mapping['cache'].setdefault("revsetcache", {})
        if raw in revsetcache:
            revs = revsetcache[raw]
        else:
            revs = query(raw)
            revs = list([str(r) for r in revs])
            revsetcache[raw] = revs

    return templatekw.showlist("revision", revs, **mapping)

def rstdoc(context, mapping, args):
    """:rstdoc(text, style): Format ReStructuredText."""
    if len(args) != 2:
        # i18n: "rstdoc" is a keyword
        raise error.ParseError(_("rstdoc expects two arguments"))

    text = stringify(args[0][0](context, mapping, args[0][1]))
    style = stringify(args[1][0](context, mapping, args[1][1]))

    return minirst.format(text, style=style, keep=['verbose'])

def shortest(context, mapping, args):
    """:shortest(node, minlength=4): Obtain the shortest representation of
    a node."""
    if not (1 <= len(args) <= 2):
        # i18n: "shortest" is a keyword
        raise error.ParseError(_("shortest() expects one or two arguments"))

    node = stringify(args[0][0](context, mapping, args[0][1]))

    minlength = 4
    if len(args) > 1:
        minlength = int(args[1][1])

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

def strip(context, mapping, args):
    """:strip(text[, chars]): Strip characters from a string."""
    if not (1 <= len(args) <= 2):
        # i18n: "strip" is a keyword
        raise error.ParseError(_("strip expects one or two arguments"))

    text = stringify(args[0][0](context, mapping, args[0][1]))
    if len(args) == 2:
        chars = stringify(args[1][0](context, mapping, args[1][1]))
        return text.strip(chars)
    return text.strip()

def sub(context, mapping, args):
    """:sub(pattern, replacement, expression): Perform text substitution
    using regular expressions."""
    if len(args) != 3:
        # i18n: "sub" is a keyword
        raise error.ParseError(_("sub expects three arguments"))

    pat = stringify(args[0][0](context, mapping, args[0][1]))
    rpl = stringify(args[1][0](context, mapping, args[1][1]))
    src = stringify(_evalifliteral(args[2], context, mapping))
    yield re.sub(pat, rpl, src)

def startswith(context, mapping, args):
    """:startswith(pattern, text): Returns the value from the "text" argument
    if it begins with the content from the "pattern" argument."""
    if len(args) != 2:
        # i18n: "startswith" is a keyword
        raise error.ParseError(_("startswith expects two arguments"))

    patn = stringify(args[0][0](context, mapping, args[0][1]))
    text = stringify(args[1][0](context, mapping, args[1][1]))
    if text.startswith(patn):
        return text
    return ''


def word(context, mapping, args):
    """:word(number, text[, separator]): Return the nth word from a string."""
    if not (2 <= len(args) <= 3):
        # i18n: "word" is a keyword
        raise error.ParseError(_("word expects two or three arguments, got %d")
                               % len(args))

    try:
        num = int(stringify(args[0][0](context, mapping, args[0][1])))
    except ValueError:
        # i18n: "word" is a keyword
        raise error.ParseError(_("word expects an integer index"))
    text = stringify(args[1][0](context, mapping, args[1][1]))
    if len(args) == 3:
        splitter = stringify(args[2][0](context, mapping, args[2][1]))
    else:
        splitter = None

    tokens = text.split(splitter)
    if num >= len(tokens):
        return ''
    else:
        return tokens[num]

# methods to interpret function arguments or inner expressions (e.g. {_(x)})
exprmethods = {
    "integer": lambda e, c: (runinteger, e[1]),
    "string": lambda e, c: (runstring, e[1]),
    "rawstring": lambda e, c: (runrawstring, e[1]),
    "symbol": lambda e, c: (runsymbol, e[1]),
    "group": lambda e, c: compileexp(e[1], c, exprmethods),
#    ".": buildmember,
    "|": buildfilter,
    "%": buildmap,
    "func": buildfunc,
    }

# methods to interpret top-level template (e.g. {x}, {x|_}, {x % "y"})
methods = exprmethods.copy()
methods["integer"] = exprmethods["symbol"]  # '{1}' as variable

funcs = {
    "date": date,
    "diff": diff,
    "fill": fill,
    "get": get,
    "if": if_,
    "ifcontains": ifcontains,
    "ifeq": ifeq,
    "indent": indent,
    "join": join,
    "label": label,
    "pad": pad,
    "revset": revset,
    "rstdoc": rstdoc,
    "shortest": shortest,
    "startswith": startswith,
    "strip": strip,
    "sub": sub,
    "word": word,
}

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
    '''unwrap quotes'''
    if len(s) < 2 or s[0] != s[-1]:
        raise SyntaxError(_('unmatched quotes'))
    # de-backslash-ify only <\">. it is invalid syntax in non-string part of
    # template, but we are likely to escape <"> in quoted string and it was
    # accepted before, thanks to issue4290. <\\"> is unmodified because it
    # is ambiguous and it was processed as such before 2.8.1.
    #
    #  template  result
    #  --------- ------------------------
    #  {\"\"}    parse error
    #  "{""}"    {""} -> <>
    #  "{\"\"}"  {""} -> <>
    #  {"\""}    {"\""} -> <">
    #  '{"\""}'  {"\""} -> <">
    #  "{"\""}"  parse error (don't care)
    q = s[0]
    return s[1:-1].replace('\\\\' + q, '\\\\\\' + q).replace('\\' + q, q)

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

    def __init__(self, loader, filters={}, defaults={}):
        self._loader = loader
        self._filters = filters
        self._defaults = defaults
        self._cache = {}

    def _load(self, t):
        '''load, parse, and cache a template'''
        if t not in self._cache:
            self._cache[t] = compiletemplate(self._loader(t), self)
        return self._cache[t]

    def process(self, t, mapping):
        '''Perform expansion. t is name of map element to expand.
        mapping contains added elements for use during expansion. Is a
        generator.'''
        return _flatten(runtemplate(self, mapping, self._load(t)))

engines = {'default': engine}

def stylelist():
    paths = templatepaths()
    if not paths:
        return _('no templates found, try `hg debuginstall` for more info')
    dirlist =  os.listdir(paths[0])
    stylelist = []
    for file in dirlist:
        split = file.split(".")
        if split[0] == "map-cmdline":
            stylelist.append(split[1])
    return ", ".join(sorted(stylelist))

class TemplateNotFound(util.Abort):
    pass

class templater(object):

    def __init__(self, mapfile, filters={}, defaults={}, cache={},
                 minchunk=1024, maxchunk=65536):
        '''set up template engine.
        mapfile is name of file to read map definitions from.
        filters is dict of functions. each transforms a value into another.
        defaults is dict of default map definitions.'''
        self.mapfile = mapfile or 'template'
        self.cache = cache.copy()
        self.map = {}
        if mapfile:
            self.base = os.path.dirname(mapfile)
        else:
            self.base = ''
        self.filters = templatefilters.filters.copy()
        self.filters.update(filters)
        self.defaults = defaults
        self.minchunk, self.maxchunk = minchunk, maxchunk
        self.ecache = {}

        if not mapfile:
            return
        if not os.path.exists(mapfile):
            raise util.Abort(_("style '%s' not found") % mapfile,
                             hint=_("available styles: %s") % stylelist())

        conf = config.config(includepaths=templatepaths())
        conf.read(mapfile)

        for key, val in conf[''].items():
            if not val:
                raise SyntaxError(_('%s: missing value') % conf.source('', key))
            if val[0] in "'\"":
                try:
                    self.cache[key] = unquotestring(val)
                except SyntaxError, inst:
                    raise SyntaxError('%s: %s' %
                                      (conf.source('', key), inst.args[0]))
            else:
                val = 'default', val
                if ':' in val[1]:
                    val = val[1].split(':', 1)
                self.map[key] = val[0], os.path.join(self.base, val[1])

    def __contains__(self, key):
        return key in self.cache or key in self.map

    def load(self, t):
        '''Get the template for the given template name. Use a local cache.'''
        if t not in self.cache:
            try:
                self.cache[t] = util.readfile(self.map[t][1])
            except KeyError, inst:
                raise TemplateNotFound(_('"%s" not in template map') %
                                       inst.args[0])
            except IOError, inst:
                raise IOError(inst.args[0], _('template file %s: %s') %
                              (self.map[t][1], inst.args[1]))
        return self.cache[t]

    def __call__(self, t, **mapping):
        ttype = t in self.map and self.map[t][0] or 'default'
        if ttype not in self.ecache:
            self.ecache[ttype] = engines[ttype](self.load,
                                                 self.filters, self.defaults)
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

# tell hggettext to extract docstrings from these functions:
i18nfunctions = funcs.values()
