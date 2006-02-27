import re
from demandload import demandload
from i18n import gettext as _
demandload(globals(), "cStringIO cgi os time urllib util")

esctable = {
    '\\': '\\',
    'r': '\r',
    't': '\t',
    'n': '\n',
    'v': '\v',
    }

def parsestring(s, quoted=True):
    fp = cStringIO.StringIO()
    if quoted:
        first = s[0]
        if len(s) < 2: raise SyntaxError(_('string too short'))
        if first not in "'\"": raise SyntaxError(_('invalid quote'))
        if s[-1] != first: raise SyntaxError(_('unmatched quotes'))
        s = s[1:-1]
    escape = False
    for c in s:
        if escape:
            fp.write(esctable.get(c, c))
            escape = False
        elif c == '\\': escape = True
        elif quoted and c == first: raise SyntaxError(_('string ends early'))
        else: fp.write(c)
    if escape: raise SyntaxError(_('unterminated escape'))
    return fp.getvalue()

class templater(object):
    def __init__(self, mapfile, filters={}, defaults={}):
        self.mapfile = mapfile or 'template'
        self.cache = {}
        self.map = {}
        self.base = (mapfile and os.path.dirname(mapfile)) or ''
        self.filters = filters
        self.defaults = defaults

        if not mapfile:
            return
        i = 0
        for l in file(mapfile):
            l = l.rstrip('\r\n')
            i += 1
            if l.startswith('#') or not l.strip(): continue
            m = re.match(r'([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*(.+)$', l)
            if m:
                key, val = m.groups()
                if val[0] in "'\"":
                    try:
                        self.cache[key] = parsestring(val)
                    except SyntaxError, inst:
                        raise SyntaxError('%s:%s: %s' %
                                          (mapfile, i, inst.args[0]))
                else:
                    self.map[key] = os.path.join(self.base, val)
            else:
                raise SyntaxError(_("%s:%s: parse error") % (mapfile, i))

    def __contains__(self, key):
        return key in self.cache

    def __call__(self, t, **map):
        m = self.defaults.copy()
        m.update(map)
        try:
            tmpl = self.cache[t]
        except KeyError:
            try:
                tmpl = self.cache[t] = file(self.map[t]).read()
            except IOError, inst:
                raise IOError(inst.args[0], _('template file %s: %s') %
                              (self.map[t], inst.args[1]))
        return self.template(tmpl, self.filters, **m)

    template_re = re.compile(r"[#{]([a-zA-Z_][a-zA-Z0-9_]*)"
                             r"((%[a-zA-Z_][a-zA-Z0-9_]*)*)"
                             r"((\|[a-zA-Z_][a-zA-Z0-9_]*)*)[#}]")

    def template(self, tmpl, filters={}, **map):
        lm = map.copy()
        while tmpl:
            m = self.template_re.search(tmpl)
            if m:
                start, end = m.span(0)
                s, e = tmpl[start], tmpl[end - 1]
                key = m.group(1)
                if ((s == '#' and e != '#') or (s == '{' and e != '}')):
                    raise SyntaxError(_("'%s'/'%s' mismatch expanding '%s'") %
                                      (s, e, key))
                if start:
                    yield tmpl[:start]
                v = map.get(key, "")
                v = callable(v) and v(**map) or v

                format = m.group(2)
                fl = m.group(4)

                if format:
                    q = v.__iter__
                    for i in q():
                        lm.update(i)
                        yield self(format[1:], **lm)

                    v = ""

                elif fl:
                    for f in fl.split("|")[1:]:
                        v = filters[f](v)

                yield v
                tmpl = tmpl[end:]
            else:
                yield tmpl
                break

agescales = [("second", 1),
             ("minute", 60),
             ("hour", 3600),
             ("day", 3600 * 24),
             ("week", 3600 * 24 * 7),
             ("month", 3600 * 24 * 30),
             ("year", 3600 * 24 * 365)]

agescales.reverse()

def age(x):
    def plural(t, c):
        if c == 1:
            return t
        return t + "s"
    def fmt(t, c):
        return "%d %s" % (c, plural(t, c))

    now = time.time()
    then = x[0]
    delta = max(1, int(now - then))

    for t, s in agescales:
        n = delta / s
        if n >= 2 or s == 1:
            return fmt(t, n)

def isodate(date):
    return util.datestr(date, format='%Y-%m-%d %H:%M')

def nl2br(text):
    return text.replace('\n', '<br/>\n')

def obfuscate(text):
    return ''.join(['&#%d;' % ord(c) for c in text])

def domain(author):
    f = author.find('@')
    if f == -1: return ''
    author = author[f+1:]
    f = author.find('>')
    if f >= 0: author = author[:f]
    return author

def person(author):
    f = author.find('<')
    if f == -1: return util.shortuser(author)
    return author[:f].rstrip()

common_filters = {
    "addbreaks": nl2br,
    "age": age,
    "date": lambda x: util.datestr(x),
    "domain": domain,
    "escape": lambda x: cgi.escape(x, True),
    "firstline": (lambda x: x.splitlines(1)[0]),
    "isodate": isodate,
    "obfuscate": obfuscate,
    "permissions": (lambda x: x and "-rwxr-xr-x" or "-rw-r--r--"),
    "person": person,
    "rfc822date": lambda x: util.datestr(x, "%a, %d %b %Y %H:%M:%S"),
    "short": (lambda x: x[:12]),
    "strip": lambda x: x.strip(),
    "urlescape": urllib.quote,
    "user": util.shortuser,
    }

def templatepath(name=None):
    for f in 'templates', '../templates':
        fl = f.split('/')
        if name: fl.append(name)
        p = os.path.join(os.path.dirname(__file__), *fl)
        if (name and os.path.exists(p)) or os.path.isdir(p):
            return os.path.normpath(p)
