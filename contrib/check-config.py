#!/usr/bin/env python
#
# check-config - a config flag documentation checker for Mercurial
#
# Copyright 2015 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import, print_function
import re
import sys

foundopts = {}
documented = {}
allowinconsistent = set()

configre = re.compile(r'''
    # Function call
    ui\.config(?P<ctype>|int|bool|list)\(
        # First argument.
        ['"](?P<section>\S+)['"],\s*
        # Second argument
        ['"](?P<option>\S+)['"](,\s+
        (?:default=)?(?P<default>\S+?))?
    \)''', re.VERBOSE | re.MULTILINE)

configwithre = re.compile('''
    ui\.config(?P<ctype>with)\(
        # First argument is callback function. This doesn't parse robustly
        # if it is e.g. a function call.
        [^,]+,\s*
        ['"](?P<section>\S+)['"],\s*
        ['"](?P<option>\S+)['"](,\s+
        (?:default=)?(?P<default>\S+?))?
    \)''', re.VERBOSE | re.MULTILINE)

configpartialre = (r"""ui\.config""")

ignorere = re.compile(r'''
    \#\s(?P<reason>internal|experimental|deprecated|developer|inconsistent)\s
    config:\s(?P<config>\S+\.\S+)$
    ''', re.VERBOSE | re.MULTILINE)

def main(args):
    for f in args:
        sect = ''
        prevname = ''
        confsect = ''
        carryover = ''
        linenum = 0
        for l in open(f):
            linenum += 1

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
            m = re.match(r'^\s+(?:#\s*)?(\S+) = ', l)
            if m:
                name = confsect + '.' + m.group(1)
                documented[name] = 1

            # like the bugzilla extension
            m = re.match(r'^\s*(\S+\.\S+)$', l)
            if m:
                documented[m.group(1)] = 1

            # like convert
            m = re.match(r'^\s*:(\S+\.\S+):\s+', l)
            if m:
                documented[m.group(1)] = 1

            # quoted in help or docstrings
            m = re.match(r'.*?``(\S+\.\S+)``', l)
            if m:
                documented[m.group(1)] = 1

            # look for ignore markers
            m = ignorere.search(l)
            if m:
                if m.group('reason') == 'inconsistent':
                    allowinconsistent.add(m.group('config'))
                else:
                    documented[m.group('config')] = 1

            # look for code-like bits
            line = carryover + l
            m = configre.search(line) or configwithre.search(line)
            if m:
                ctype = m.group('ctype')
                if not ctype:
                    ctype = 'str'
                name = m.group('section') + "." + m.group('option')
                default = m.group('default')
                if default in (None, 'False', 'None', '0', '[]', '""', "''"):
                    default = ''
                if re.match('[a-z.]+$', default):
                    default = '<variable>'
                if (name in foundopts and (ctype, default) != foundopts[name]
                    and name not in allowinconsistent):
                    print(l.rstrip())
                    print("conflict on %s: %r != %r" % (name, (ctype, default),
                                                        foundopts[name]))
                    print("at %s:%d:" % (f, linenum))
                foundopts[name] = (ctype, default)
                carryover = ''
            else:
                m = re.search(configpartialre, line)
                if m:
                    carryover = line
                else:
                    carryover = ''

    for name in sorted(foundopts):
        if name not in documented:
            if not (name.startswith("devel.") or
                    name.startswith("experimental.") or
                    name.startswith("debug.")):
                ctype, default = foundopts[name]
                if default:
                    default = ' [%s]' % default
                print("undocumented: %s (%s)%s" % (name, ctype, default))

if __name__ == "__main__":
    if len(sys.argv) > 1:
        sys.exit(main(sys.argv[1:]))
    else:
        sys.exit(main([l.rstrip() for l in sys.stdin]))
