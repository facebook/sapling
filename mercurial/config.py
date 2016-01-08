# config.py - configuration parsing for Mercurial
#
#  Copyright 2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import os

from .i18n import _
from . import (
    error,
    util,
)

class config(object):
    def __init__(self, data=None, includepaths=[]):
        self._data = {}
        self._source = {}
        self._unset = []
        self._includepaths = includepaths
        if data:
            for k in data._data:
                self._data[k] = data[k].copy()
            self._source = data._source.copy()
    def copy(self):
        return config(self)
    def __contains__(self, section):
        return section in self._data
    def hasitem(self, section, item):
        return item in self._data.get(section, {})
    def __getitem__(self, section):
        return self._data.get(section, {})
    def __iter__(self):
        for d in self.sections():
            yield d
    def update(self, src):
        for s, n in src._unset:
            if s in self and n in self._data[s]:
                del self._data[s][n]
                del self._source[(s, n)]
        for s in src:
            if s not in self:
                self._data[s] = util.sortdict()
            self._data[s].update(src._data[s])
        self._source.update(src._source)
    def get(self, section, item, default=None):
        return self._data.get(section, {}).get(item, default)

    def backup(self, section, item):
        """return a tuple allowing restore to reinstall a previous value

        The main reason we need it is because it handles the "no data" case.
        """
        try:
            value = self._data[section][item]
            source = self.source(section, item)
            return (section, item, value, source)
        except KeyError:
            return (section, item)

    def source(self, section, item):
        return self._source.get((section, item), "")
    def sections(self):
        return sorted(self._data.keys())
    def items(self, section):
        return self._data.get(section, {}).items()
    def set(self, section, item, value, source=""):
        if section not in self:
            self._data[section] = util.sortdict()
        self._data[section][item] = value
        if source:
            self._source[(section, item)] = source

    def restore(self, data):
        """restore data returned by self.backup"""
        if len(data) == 4:
            # restore old data
            section, item, value, source = data
            self._data[section][item] = value
            self._source[(section, item)] = source
        else:
            # no data before, remove everything
            section, item = data
            if section in self._data:
                self._data[section].pop(item, None)
            self._source.pop((section, item), None)

    def parse(self, src, data, sections=None, remap=None, include=None):
        sectionre = util.re.compile(r'\[([^\[]+)\]')
        itemre = util.re.compile(r'([^=\s][^=]*?)\s*=\s*(.*\S|)')
        contre = util.re.compile(r'\s+(\S|\S.*\S)\s*$')
        emptyre = util.re.compile(r'(;|#|\s*$)')
        commentre = util.re.compile(r'(;|#)')
        unsetre = util.re.compile(r'%unset\s+(\S+)')
        includere = util.re.compile(r'%include\s+(\S|\S.*\S)\s*$')
        section = ""
        item = None
        line = 0
        cont = False

        for l in data.splitlines(True):
            line += 1
            if line == 1 and l.startswith('\xef\xbb\xbf'):
                # Someone set us up the BOM
                l = l[3:]
            if cont:
                if commentre.match(l):
                    continue
                m = contre.match(l)
                if m:
                    if sections and section not in sections:
                        continue
                    v = self.get(section, item) + "\n" + m.group(1)
                    self.set(section, item, v, "%s:%d" % (src, line))
                    continue
                item = None
                cont = False
            m = includere.match(l)

            if m and include:
                expanded = util.expandpath(m.group(1))
                includepaths = [os.path.dirname(src)] + self._includepaths

                for base in includepaths:
                    inc = os.path.normpath(os.path.join(base, expanded))

                    try:
                        include(inc, remap=remap, sections=sections)
                        break
                    except IOError as inst:
                        if inst.errno != errno.ENOENT:
                            raise error.ParseError(_("cannot include %s (%s)")
                                                   % (inc, inst.strerror),
                                                   "%s:%s" % (src, line))
                continue
            if emptyre.match(l):
                continue
            m = sectionre.match(l)
            if m:
                section = m.group(1)
                if remap:
                    section = remap.get(section, section)
                if section not in self:
                    self._data[section] = util.sortdict()
                continue
            m = itemre.match(l)
            if m:
                item = m.group(1)
                cont = True
                if sections and section not in sections:
                    continue
                self.set(section, item, m.group(2), "%s:%d" % (src, line))
                continue
            m = unsetre.match(l)
            if m:
                name = m.group(1)
                if sections and section not in sections:
                    continue
                if self.get(section, name) is not None:
                    del self._data[section][name]
                self._unset.append((section, name))
                continue

            raise error.ParseError(l.rstrip(), ("%s:%s" % (src, line)))

    def read(self, path, fp=None, sections=None, remap=None):
        if not fp:
            fp = util.posixfile(path)
        self.parse(path, fp.read(), sections, remap, self.read)
