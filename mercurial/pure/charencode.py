# charencode.py - miscellaneous character encoding
#
#  Copyright 2005-2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

def asciilower(s):
    '''convert a string to lowercase if ASCII

    Raises UnicodeDecodeError if non-ASCII characters are found.'''
    s.decode('ascii')
    return s.lower()

def asciiupper(s):
    '''convert a string to uppercase if ASCII

    Raises UnicodeDecodeError if non-ASCII characters are found.'''
    s.decode('ascii')
    return s.upper()
