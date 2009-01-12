"""
error.py - Mercurial exceptions

This allows us to catch exceptions at higher levels without forcing imports

Copyright 2005-2008 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

# Do not import anything here, please

class RevlogError(Exception):
    pass

class LookupError(RevlogError, KeyError):
    def __init__(self, name, index, message):
        self.name = name
        if isinstance(name, str) and len(name) == 20:
            from node import short
            name = short(name)
        RevlogError.__init__(self, '%s@%s: %s' % (index, name, message))

    def __str__(self):
        return RevlogError.__str__(self)
