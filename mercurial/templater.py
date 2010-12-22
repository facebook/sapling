# templater.py - template expansion for output
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import sys, os
import util, config, templatefilters, parser, error

# template parsing

elements = {
    "(": (20, ("group", 1, ")"), ("func", 1, ")")),
    ",": (2, None, ("list", 2)),
    "|": (5, None, ("|", 5)),
    "%": (6, None, ("%", 6)),
    ")": (0, None, None),
    "symbol": (0, ("symbol",), None),
    "string": (0, ("string",), None),
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
                decode = lambda x: x
            else:
                decode = lambda x: x.decode('string-escape')
            pos += 1
            s = pos
            while pos < end: # find closing quote
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
    data[2] = pos
    yield ('end', None, pos)

def compiletemplate(tmpl, context):
    parsed = []
    pos, stop = 0, len(tmpl)
    p = parser.parser(tokenizer, elements)

    while pos < stop:
        n = tmpl.find('{', pos)
        if n < 0:
            parsed.append(("string", tmpl[pos:]))
            break
        if n > 0 and tmpl[n - 1] == '\\':
            # escaped
            parsed.append(("string", tmpl[pos:n - 1] + "{"))
            pos = n + 1
            continue
        if n > pos:
            parsed.append(("string", tmpl[pos:n]))

        pd = [tmpl, n + 1, stop]
        parsed.append(p.parse(pd))
        pos = pd[2]

    return [compileexp(e, context) for e in parsed]

def compileexp(exp, context):
    t = exp[0]
    if t in methods:
        return methods[t](exp, context)
    raise error.ParseError(_("unknown method '%s'") % t)

# template evaluation

def getsymbol(exp):
    if exp[0] == 'symbol':
        return exp[1]
    raise error.ParseError(_("expected a symbol"))

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
    if exp[0] == 'string':
        return compiletemplate(exp[1], context)
    if exp[0] == 'symbol':
        return context._load(exp[1])
    raise error.ParseError(_("expected template specifier"))

def runstring(context, mapping, data):
    return data

def runsymbol(context, mapping, key):
    v = mapping.get(key)
    if v is None:
        v = context._defaults.get(key, '')
    if hasattr(v, '__call__'):
        return v(**mapping)
    return v

def buildfilter(exp, context):
    func, data = compileexp(exp[1], context)
    filt = getfilter(exp[2], context)
    return (runfilter, (func, data, filt))

def runfilter(context, mapping, data):
    func, data, filt = data
    return filt(func(context, mapping, data))

def buildmap(exp, context):
    func, data = compileexp(exp[1], context)
    ctmpl = gettemplate(exp[2], context)
    return (runmap, (func, data, ctmpl))

def runmap(context, mapping, data):
    func, data, ctmpl = data
    d = func(context, mapping, data)
    lm = mapping.copy()

    for i in d:
        if isinstance(i, dict):
            lm.update(i)
            for f, d in ctmpl:
                yield f(context, lm, d)
        else:
            # v is not an iterable of dicts, this happen when 'key'
            # has been fully expanded already and format is useless.
            # If so, return the expanded value.
            yield i

def buildfunc(exp, context):
    n = getsymbol(exp[1])
    args = [compileexp(x, context) for x in getlist(exp[2])]
    if n in context._filters:
        if len(args) != 1:
            raise error.ParseError(_("filter %s expects one argument") % n)
        f = context._filters[n]
        return (runfilter, (args[0][0], args[0][1], f))
    elif n in context._funcs:
        f = context._funcs[n]
        return (f, args)

methods = {
    "string": lambda e, c: (runstring, e[1]),
    "symbol": lambda e, c: (runsymbol, e[1]),
    "group": lambda e, c: compileexp(e[1], c),
#    ".": buildmember,
    "|": buildfilter,
    "%": buildmap,
    "func": buildfunc,
    }

# template engine

path = ['templates', '../templates']
stringify = templatefilters.stringify

def _flatten(thing):
    '''yield a single stream from a possibly nested set of iterators'''
    if isinstance(thing, str):
        yield thing
    elif not hasattr(thing, '__iter__'):
        if thing is not None:
            yield str(thing)
    else:
        for i in thing:
            if isinstance(i, str):
                yield i
            elif not hasattr(i, '__iter__'):
                if i is not None:
                    yield str(i)
            elif i is not None:
                for j in _flatten(i):
                    yield j

def parsestring(s, quoted=True):
    '''parse a string using simple c-like syntax.
    string must be in quotes if quoted is True.'''
    if quoted:
        if len(s) < 2 or s[0] != s[-1]:
            raise SyntaxError(_('unmatched quotes'))
        return s[1:-1].decode('string_escape')

    return s.decode('string_escape')

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
        return _flatten(func(self, mapping, data) for func, data in
                         self._load(t))

engines = {'default': engine}

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
        self.base = (mapfile and os.path.dirname(mapfile)) or ''
        self.filters = templatefilters.filters.copy()
        self.filters.update(filters)
        self.defaults = defaults
        self.minchunk, self.maxchunk = minchunk, maxchunk
        self.ecache = {}

        if not mapfile:
            return
        if not os.path.exists(mapfile):
            raise util.Abort(_('style not found: %s') % mapfile)

        conf = config.config()
        conf.read(mapfile)

        for key, val in conf[''].items():
            if val[0] in "'\"":
                try:
                    self.cache[key] = parsestring(val)
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
        if not t in self.cache:
            try:
                self.cache[t] = open(self.map[t][1]).read()
            except KeyError, inst:
                raise util.Abort(_('"%s" not in template map') % inst.args[0])
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

def templatepath(name=None):
    '''return location of template file or directory (if no name).
    returns None if not found.'''
    normpaths = []

    # executable version (py2exe) doesn't support __file__
    if hasattr(sys, 'frozen'):
        module = sys.executable
    else:
        module = __file__
    for f in path:
        if f.startswith('/'):
            p = f
        else:
            fl = f.split('/')
            p = os.path.join(os.path.dirname(module), *fl)
        if name:
            p = os.path.join(p, name)
        if name and os.path.exists(p):
            return os.path.normpath(p)
        elif os.path.isdir(p):
            normpaths.append(os.path.normpath(p))

    return normpaths

def stylemap(styles, paths=None):
    """Return path to mapfile for a given style.

    Searches mapfile in the following locations:
    1. templatepath/style/map
    2. templatepath/map-style
    3. templatepath/map
    """

    if paths is None:
        paths = templatepath()
    elif isinstance(paths, str):
        paths = [paths]

    if isinstance(styles, str):
        styles = [styles]

    for style in styles:
        if not style:
            continue
        locations = [os.path.join(style, 'map'), 'map-' + style]
        locations.append('map')

        for path in paths:
            for location in locations:
                mapfile = os.path.join(path, location)
                if os.path.isfile(mapfile):
                    return style, mapfile

    raise RuntimeError("No hgweb templates found in %r" % paths)
