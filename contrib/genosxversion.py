#!/usr/bin/env python2
from __future__ import absolute_import, print_function

import argparse
import json
import os
import subprocess
import sys

# Always load hg libraries from the hg we can find on $PATH.
hglib = json.loads(subprocess.check_output(
    ['hg', 'debuginstall', '-Tjson']))[0]['hgmodules']
sys.path.insert(0, os.path.dirname(hglib))

from mercurial import util

ap = argparse.ArgumentParser()
ap.add_argument('--paranoid',
                action='store_true',
                help=("Be paranoid about how version numbers compare and "
                      "produce something that's more likely to sort "
                      "reasonably."))
ap.add_argument('--selftest', action='store_true', help='Run self-tests.')
ap.add_argument('versionfile', help='Path to a valid mercurial __version__.py')

def paranoidver(ver):
    """Given an hg version produce something that distutils can sort.

    Some Mac package management systems use distutils code in order to
    figure out upgrades, which makes life difficult. The test case is
    a reduced version of code in the Munki tool used by some large
    organizations to centrally manage OS X packages, which is what
    inspired this kludge.

    >>> paranoidver('3.4')
    '3.4.0'
    >>> paranoidver('3.4.2')
    '3.4.2'
    >>> paranoidver('3.0-rc+10')
    '2.9.9999-rc+10'
    >>> paranoidver('4.2+483-5d44d7d4076e')
    '4.2.0+483-5d44d7d4076e'
    >>> paranoidver('4.2.1+598-48d1e1214d8c')
    '4.2.1+598-48d1e1214d8c'
    >>> paranoidver('4.3-rc')
    '4.2.9999-rc'
    >>> paranoidver('4.3')
    '4.3.0'
    >>> from distutils import version
    >>> class LossyPaddedVersion(version.LooseVersion):
    ...     '''Subclass version.LooseVersion to compare things like
    ...     "10.6" and "10.6.0" as equal'''
    ...     def __init__(self, s):
    ...             self.parse(s)
    ...
    ...     def _pad(self, version_list, max_length):
    ...         'Pad a version list by adding extra 0 components to the end'
    ...         # copy the version_list so we don't modify it
    ...         cmp_list = list(version_list)
    ...         while len(cmp_list) < max_length:
    ...             cmp_list.append(0)
    ...         return cmp_list
    ...
    ...     def __cmp__(self, other):
    ...         if isinstance(other, str):
    ...             other = MunkiLooseVersion(other)
    ...         max_length = max(len(self.version), len(other.version))
    ...         self_cmp_version = self._pad(self.version, max_length)
    ...         other_cmp_version = self._pad(other.version, max_length)
    ...         return cmp(self_cmp_version, other_cmp_version)
    >>> def testver(older, newer):
    ...   o = LossyPaddedVersion(paranoidver(older))
    ...   n = LossyPaddedVersion(paranoidver(newer))
    ...   return o < n
    >>> testver('3.4', '3.5')
    True
    >>> testver('3.4.0', '3.5-rc')
    True
    >>> testver('3.4-rc', '3.5')
    True
    >>> testver('3.4-rc+10-deadbeef', '3.5')
    True
    >>> testver('3.4.2', '3.5-rc')
    True
    >>> testver('3.4.2', '3.5-rc+10-deadbeef')
    True
    >>> testver('4.2+483-5d44d7d4076e', '4.2.1+598-48d1e1214d8c')
    True
    >>> testver('4.3-rc', '4.3')
    True
    >>> testver('4.3', '4.3-rc')
    False
    """
    major, minor, micro, extra = util.versiontuple(ver, n=4)
    if micro is None:
        micro = 0
    if extra:
        if extra.startswith('rc'):
            if minor == 0:
                major -= 1
                minor = 9
            else:
                minor -= 1
            micro = 9999
            extra = '-' + extra
        else:
            extra = '+' + extra
    else:
        extra = ''
    return '%d.%d.%d%s' % (major, minor, micro, extra)

def main(argv):
    opts = ap.parse_args(argv[1:])
    if opts.selftest:
        import doctest
        doctest.testmod()
        return
    with open(opts.versionfile) as f:
        for l in f:
            if l.startswith('version = '):
                # version number is entire line minus the quotes
                ver = l[len('version = ') + 1:-2]
                break
    if opts.paranoid:
        print(paranoidver(ver))
    else:
        print(ver)

if __name__ == '__main__':
    main(sys.argv)
