# formatter.py - generic output formatting for mercurial
#
# Copyright 2012 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

from .i18n import _
from .node import (
    hex,
    short,
)

from . import (
    encoding,
    error,
    templatekw,
    templater,
    util,
)

pickle = util.pickle

class baseformatter(object):
    def __init__(self, ui, topic, opts):
        self._ui = ui
        self._topic = topic
        self._style = opts.get("style")
        self._template = opts.get("template")
        self._item = None
        # function to convert node to string suitable for this output
        self.hexfunc = hex
    def __nonzero__(self):
        '''return False if we're not doing real templating so we can
        skip extra work'''
        return True
    def _showitem(self):
        '''show a formatted item once all data is collected'''
        pass
    def startitem(self):
        '''begin an item in the format list'''
        if self._item is not None:
            self._showitem()
        self._item = {}
    @staticmethod
    def formatdate(date, fmt='%a %b %d %H:%M:%S %Y %1%2'):
        '''convert date tuple to appropriate format'''
        return date
    @staticmethod
    def formatdict(data, key='key', value='value', fmt='%s=%s', sep=' '):
        '''convert dict or key-value pairs to appropriate dict format'''
        # use plain dict instead of util.sortdict so that data can be
        # serialized as a builtin dict in pickle output
        return dict(data)
    @staticmethod
    def formatlist(data, name, fmt='%s', sep=' '):
        '''convert iterable to appropriate list format'''
        return list(data)
    def data(self, **data):
        '''insert data into item that's not shown in default output'''
        self._item.update(data)
    def write(self, fields, deftext, *fielddata, **opts):
        '''do default text output while assigning data to item'''
        fieldkeys = fields.split()
        assert len(fieldkeys) == len(fielddata)
        self._item.update(zip(fieldkeys, fielddata))
    def condwrite(self, cond, fields, deftext, *fielddata, **opts):
        '''do conditional write (primarily for plain formatter)'''
        fieldkeys = fields.split()
        assert len(fieldkeys) == len(fielddata)
        self._item.update(zip(fieldkeys, fielddata))
    def plain(self, text, **opts):
        '''show raw text for non-templated mode'''
        pass
    def end(self):
        '''end output for the formatter'''
        if self._item is not None:
            self._showitem()

def _iteritems(data):
    '''iterate key-value pairs in stable order'''
    if isinstance(data, dict):
        return sorted(data.iteritems())
    return data

class plainformatter(baseformatter):
    '''the default text output scheme'''
    def __init__(self, ui, topic, opts):
        baseformatter.__init__(self, ui, topic, opts)
        if ui.debugflag:
            self.hexfunc = hex
        else:
            self.hexfunc = short
    def __nonzero__(self):
        return False
    def startitem(self):
        pass
    @staticmethod
    def formatdate(date, fmt='%a %b %d %H:%M:%S %Y %1%2'):
        '''stringify date tuple in the given format'''
        return util.datestr(date, fmt)
    @staticmethod
    def formatdict(data, key='key', value='value', fmt='%s=%s', sep=' '):
        '''stringify key-value pairs separated by sep'''
        return sep.join(fmt % (k, v) for k, v in _iteritems(data))
    @staticmethod
    def formatlist(data, name, fmt='%s', sep=' '):
        '''stringify iterable separated by sep'''
        return sep.join(fmt % e for e in data)
    def data(self, **data):
        pass
    def write(self, fields, deftext, *fielddata, **opts):
        self._ui.write(deftext % fielddata, **opts)
    def condwrite(self, cond, fields, deftext, *fielddata, **opts):
        '''do conditional write'''
        if cond:
            self._ui.write(deftext % fielddata, **opts)
    def plain(self, text, **opts):
        self._ui.write(text, **opts)
    def end(self):
        pass

class debugformatter(baseformatter):
    def __init__(self, ui, topic, opts):
        baseformatter.__init__(self, ui, topic, opts)
        self._ui.write("%s = [\n" % self._topic)
    def _showitem(self):
        self._ui.write("    " + repr(self._item) + ",\n")
    def end(self):
        baseformatter.end(self)
        self._ui.write("]\n")

class pickleformatter(baseformatter):
    def __init__(self, ui, topic, opts):
        baseformatter.__init__(self, ui, topic, opts)
        self._data = []
    def _showitem(self):
        self._data.append(self._item)
    def end(self):
        baseformatter.end(self)
        self._ui.write(pickle.dumps(self._data))

def _jsonifyobj(v):
    if isinstance(v, dict):
        xs = ['"%s": %s' % (encoding.jsonescape(k), _jsonifyobj(u))
              for k, u in sorted(v.iteritems())]
        return '{' + ', '.join(xs) + '}'
    elif isinstance(v, (list, tuple)):
        return '[' + ', '.join(_jsonifyobj(e) for e in v) + ']'
    elif v is None:
        return 'null'
    elif v is True:
        return 'true'
    elif v is False:
        return 'false'
    elif isinstance(v, (int, float)):
        return str(v)
    else:
        return '"%s"' % encoding.jsonescape(v)

class jsonformatter(baseformatter):
    def __init__(self, ui, topic, opts):
        baseformatter.__init__(self, ui, topic, opts)
        self._ui.write("[")
        self._ui._first = True
    def _showitem(self):
        if self._ui._first:
            self._ui._first = False
        else:
            self._ui.write(",")

        self._ui.write("\n {\n")
        first = True
        for k, v in sorted(self._item.items()):
            if first:
                first = False
            else:
                self._ui.write(",\n")
            self._ui.write('  "%s": %s' % (k, _jsonifyobj(v)))
        self._ui.write("\n }")
    def end(self):
        baseformatter.end(self)
        self._ui.write("\n]\n")

class templateformatter(baseformatter):
    def __init__(self, ui, topic, opts):
        baseformatter.__init__(self, ui, topic, opts)
        self._topic = topic
        self._t = gettemplater(ui, topic, opts.get('template', ''))
    def _showitem(self):
        g = self._t(self._topic, ui=self._ui, **self._item)
        self._ui.write(templater.stringify(g))
    @staticmethod
    def formatdict(data, key='key', value='value', fmt='%s=%s', sep=' '):
        '''build object that can be evaluated as either plain string or dict'''
        data = util.sortdict(_iteritems(data))
        def f():
            yield plainformatter.formatdict(data, key, value, fmt, sep)
        return templatekw._hybrid(f(), data, lambda k: {key: k, value: data[k]},
                                  lambda d: fmt % (d[key], d[value]))
    @staticmethod
    def formatlist(data, name, fmt='%s', sep=' '):
        '''build object that can be evaluated as either plain string or list'''
        # name is mandatory argument for now, but it could be optional if
        # we have default template keyword, e.g. {item}
        data = list(data)
        def f():
            yield plainformatter.formatlist(data, name, fmt, sep)
        return templatekw._hybrid(f(), data, lambda x: {name: x},
                                  lambda d: fmt % d[name])

def lookuptemplate(ui, topic, tmpl):
    # looks like a literal template?
    if '{' in tmpl:
        return tmpl, None

    # perhaps a stock style?
    if not os.path.split(tmpl)[0]:
        mapname = (templater.templatepath('map-cmdline.' + tmpl)
                   or templater.templatepath(tmpl))
        if mapname and os.path.isfile(mapname):
            return None, mapname

    # perhaps it's a reference to [templates]
    t = ui.config('templates', tmpl)
    if t:
        return templater.unquotestring(t), None

    if tmpl == 'list':
        ui.write(_("available styles: %s\n") % templater.stylelist())
        raise error.Abort(_("specify a template"))

    # perhaps it's a path to a map or a template
    if ('/' in tmpl or '\\' in tmpl) and os.path.isfile(tmpl):
        # is it a mapfile for a style?
        if os.path.basename(tmpl).startswith("map-"):
            return None, os.path.realpath(tmpl)
        tmpl = open(tmpl).read()
        return tmpl, None

    # constant string?
    return tmpl, None

def gettemplater(ui, topic, spec):
    tmpl, mapfile = lookuptemplate(ui, topic, spec)
    assert not (tmpl and mapfile)
    if mapfile:
        return templater.templater.frommapfile(mapfile)
    return maketemplater(ui, topic, tmpl)

def maketemplater(ui, topic, tmpl, filters=None, cache=None):
    """Create a templater from a string template 'tmpl'"""
    aliases = ui.configitems('templatealias')
    t = templater.templater(filters=filters, cache=cache, aliases=aliases)
    if tmpl:
        t.cache[topic] = tmpl
    return t

def formatter(ui, topic, opts):
    template = opts.get("template", "")
    if template == "json":
        return jsonformatter(ui, topic, opts)
    elif template == "pickle":
        return pickleformatter(ui, topic, opts)
    elif template == "debug":
        return debugformatter(ui, topic, opts)
    elif template != "":
        return templateformatter(ui, topic, opts)
    # developer config: ui.formatdebug
    elif ui.configbool('ui', 'formatdebug'):
        return debugformatter(ui, topic, opts)
    # deprecated config: ui.formatjson
    elif ui.configbool('ui', 'formatjson'):
        return jsonformatter(ui, topic, opts)
    return plainformatter(ui, topic, opts)
