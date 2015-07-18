#!/usr/bin/env python
#
# check-config - a config flag documentation checker for Mercurial
#
# Copyright 2015 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import re
import sys

foundopts = {}
documented = {}

configre = (r"""ui\.config(|int|bool|list)\(['"](\S+)['"], ?"""
            r"""['"](\S+)['"](,\s(?:default=)?(\S+?))?\)""")

def main(args):
    for f in args:
        sect = ''
        prevname = ''
        confsect = ''
        for l in open(f):

            # check topic-like bits
            m = re.match('\s*``(\S+)``', l)
            if m:
                prevname = m.group(1)
            if re.match('^\s*-+$', l):
                sect = prevname
                prevname = ''

            if sect and prevname:
                name = sect + '.' + prevname
                documented[name] = 1

            # check docstring bits
            m = re.match(r'^\s+\[(\S+)\]', l)
            if m:
                confsect = m.group(1)
                continue
            m = re.match(r'^\s+(?:#\s*)?([a-z._]+) = ', l)
            if m:
                name = confsect + '.' + m.group(1)
                documented[name] = 1

            # like the bugzilla extension
            m = re.match(r'^\s*([a-z]+\.[a-z]+)$', l)
            if m:
                documented[m.group(1)] = 1

            # quoted in help or docstrings
            m = re.match(r'.*?``([-a-z_]+\.[-a-z_]+)``', l)
            if m:
                documented[m.group(1)] = 1

            # look for ignore markers
            m = re.search(r'# (?:internal|experimental|deprecated|developer)'
                          ' config: (\S+.\S+)$', l)
            if m:
                documented[m.group(1)] = 1

            # look for code-like bits
            m = re.search(configre, l)
            if m:
                ctype = m.group(1)
                if not ctype:
                    ctype = 'str'
                name = m.group(2) + "." + m.group(3)
                default = m.group(5)
                if default in (None, 'False', 'None', '0', '[]', '""', "''"):
                    default = ''
                if re.match('[a-z.]+$', default):
                    default = '<variable>'
                if name in foundopts and (ctype, default) != foundopts[name]:
                    print l
                    print "conflict on %s: %r != %r" % (name, (ctype, default),
                                                        foundopts[name])
                foundopts[name] = (ctype, default)

    for name in sorted(foundopts):
        if name not in documented:
            if not (name.startswith("devel.") or
                    name.startswith("experimental.") or
                    name.startswith("debug.")):
                ctype, default = foundopts[name]
                if default:
                    default = ' [%s]' % default
                print "undocumented: %s (%s)%s" % (name, ctype, default)

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
