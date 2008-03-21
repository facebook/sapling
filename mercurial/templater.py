# templater.py - template expansion for output
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import _
import re, sys, os
from mercurial import util

def parsestring(s, quoted=True):
    '''parse a string using simple c-like syntax.
    string must be in quotes if quoted is True.'''
    if quoted:
        if len(s) < 2 or s[0] != s[-1]:
            raise SyntaxError(_('unmatched quotes'))
        return s[1:-1].decode('string_escape')

    return s.decode('string_escape')

class templater(object):
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

    template_re = re.compile(r"(?:(?:#(?=[\w\|%]+#))|(?:{(?=[\w\|%]+})))"
                             r"(\w+)(?:(?:%(\w+))|((?:\|\w+)*))[#}]")

    def __init__(self, mapfile, filters={}, defaults={}, cache={}):
        '''set up template engine.
        mapfile is name of file to read map definitions from.
        filters is dict of functions. each transforms a value into another.
        defaults is dict of default map definitions.'''
        self.mapfile = mapfile or 'template'
        self.cache = cache.copy()
        self.map = {}
        self.base = (mapfile and os.path.dirname(mapfile)) or ''
        self.filters = filters
        self.defaults = defaults

        if not mapfile:
            return
        if not os.path.exists(mapfile):
            raise util.Abort(_('style not found: %s') % mapfile)

        i = 0
        for l in file(mapfile):
            l = l.strip()
            i += 1
            if not l or l[0] in '#;': continue
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
        return key in self.cache or key in self.map

    def __call__(self, t, **map):
        '''perform expansion.
        t is name of map element to expand.
        map is added elements to use during expansion.'''
        if not t in self.cache:
            try:
                self.cache[t] = file(self.map[t]).read()
            except IOError, inst:
                raise IOError(inst.args[0], _('template file %s: %s') %
                              (self.map[t], inst.args[1]))
        tmpl = self.cache[t]

        while tmpl:
            m = self.template_re.search(tmpl)
            if not m:
                yield tmpl
                break

            start, end = m.span(0)
            key, format, fl = m.groups()

            if start:
                yield tmpl[:start]
            tmpl = tmpl[end:]

            if key in map:
                v = map[key]
            else:
                v = self.defaults.get(key, "")
            if callable(v):
                v = v(**map)
            if format:
                if not hasattr(v, '__iter__'):
                    raise SyntaxError(_("Error expanding '%s%s'")
                                      % (key, format))
                lm = map.copy()
                for i in v:
                    lm.update(i)
                    yield self(format, **lm)
            else:
                if fl:
                    for f in fl.split("|")[1:]:
                        v = self.filters[f](v)
                yield v

def templatepath(name=None):
    '''return location of template file or directory (if no name).
    returns None if not found.'''

    # executable version (py2exe) doesn't support __file__
    if hasattr(sys, 'frozen'):
        module = sys.executable
    else:
        module = __file__
    for f in 'templates', '../templates':
        fl = f.split('/')
        if name: fl.append(name)
        p = os.path.join(os.path.dirname(module), *fl)
        if (name and os.path.exists(p)) or os.path.isdir(p):
            return os.path.normpath(p)

def stringify(thing):
    '''turn nested template iterator into string.'''
    if hasattr(thing, '__iter__'):
        return "".join([stringify(t) for t in thing if t is not None])
    return str(thing)

