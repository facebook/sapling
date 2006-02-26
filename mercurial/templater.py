from demandload import demandload
demandload(globals(), "cgi os re time urllib util")

class templater(object):
    def __init__(self, mapfile, filters={}, defaults={}):
        self.cache = {}
        self.map = {}
        self.base = os.path.dirname(mapfile)
        self.filters = filters
        self.defaults = defaults

        for l in file(mapfile):
            m = re.match(r'(\S+)\s*=\s*"(.*)"$', l)
            if m:
                self.cache[m.group(1)] = m.group(2)
            else:
                m = re.match(r'(\S+)\s*=\s*(\S+)', l)
                if m:
                    self.map[m.group(1)] = os.path.join(self.base, m.group(2))
                else:
                    raise LookupError(_("unknown map entry '%s'") % l)

    def __call__(self, t, **map):
        m = self.defaults.copy()
        m.update(map)
        try:
            tmpl = self.cache[t]
        except KeyError:
            tmpl = self.cache[t] = file(self.map[t]).read()
        return self.template(tmpl, self.filters, **m)

    def template(self, tmpl, filters={}, **map):
        while tmpl:
            m = re.search(r"#([a-zA-Z0-9]+)((%[a-zA-Z0-9]+)*)((\|[a-zA-Z0-9]+)*)#", tmpl)
            if m:
                yield tmpl[:m.start(0)]
                v = map.get(m.group(1), "")
                v = callable(v) and v(**map) or v

                format = m.group(2)
                fl = m.group(4)

                if format:
                    q = v.__iter__
                    for i in q():
                        lm = map.copy()
                        lm.update(i)
                        yield self(format[1:], **lm)

                    v = ""

                elif fl:
                    for f in fl.split("|")[1:]:
                        v = filters[f](v)

                yield v
                tmpl = tmpl[m.end(0):]
            else:
                yield tmpl
                return

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

    scales = [["second", 1],
              ["minute", 60],
              ["hour", 3600],
              ["day", 3600 * 24],
              ["week", 3600 * 24 * 7],
              ["month", 3600 * 24 * 30],
              ["year", 3600 * 24 * 365]]

    scales.reverse()

    for t, s in scales:
        n = delta / s
        if n >= 2 or s == 1:
            return fmt(t, n)

def nl2br(text):
    return text.replace('\n', '<br/>\n')

def obfuscate(text):
    return ''.join(['&#%d;' % ord(c) for c in text])

common_filters = {
    "escape": lambda x: cgi.escape(x, True),
    "urlescape": urllib.quote,
    "strip": lambda x: x.strip(),
    "age": age,
    "date": lambda x: util.datestr(x),
    "addbreaks": nl2br,
    "obfuscate": obfuscate,
    "short": (lambda x: x[:12]),
    "firstline": (lambda x: x.splitlines(1)[0]),
    "permissions": (lambda x: x and "-rwxr-xr-x" or "-rw-r--r--"),
    "rfc822date": lambda x: util.datestr(x, "%a, %d %b %Y %H:%M:%S"),
    }
