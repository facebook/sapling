# encoding.py - character transcoding support for Mercurial
#
#  Copyright 2005-2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import, print_function

import io
import locale
import os
import unicodedata

from . import (
    error,
    policy,
    pycompat,
)

from .pure import (
    charencode as charencodepure,
)

charencode = policy.importmod(r'charencode')

isasciistr = charencode.isasciistr
asciilower = charencode.asciilower
asciiupper = charencode.asciiupper
_jsonescapeu8fast = charencode.jsonescapeu8fast

_sysstr = pycompat.sysstr

if pycompat.ispy3:
    unichr = chr

# These unicode characters are ignored by HFS+ (Apple Technote 1150,
# "Unicode Subtleties"), so we need to ignore them in some places for
# sanity.
_ignore = [unichr(int(x, 16)).encode("utf-8") for x in
           "200c 200d 200e 200f 202a 202b 202c 202d 202e "
           "206a 206b 206c 206d 206e 206f feff".split()]
# verify the next function will work
assert all(i.startswith(("\xe2", "\xef")) for i in _ignore)

def hfsignoreclean(s):
    """Remove codepoints ignored by HFS+ from s.

    >>> hfsignoreclean(u'.h\u200cg'.encode('utf-8'))
    '.hg'
    >>> hfsignoreclean(u'.h\ufeffg'.encode('utf-8'))
    '.hg'
    """
    if "\xe2" in s or "\xef" in s:
        for c in _ignore:
            s = s.replace(c, '')
    return s

# encoding.environ is provided read-only, which may not be used to modify
# the process environment
_nativeenviron = (not pycompat.ispy3 or os.supports_bytes_environ)
if not pycompat.ispy3:
    environ = os.environ  # re-exports
elif _nativeenviron:
    environ = os.environb  # re-exports
else:
    # preferred encoding isn't known yet; use utf-8 to avoid unicode error
    # and recreate it once encoding is settled
    environ = dict((k.encode(u'utf-8'), v.encode(u'utf-8'))
                   for k, v in os.environ.items())  # re-exports

_encodingfixers = {
    '646': lambda: 'ascii',
    'ANSI_X3.4-1968': lambda: 'ascii',
}

try:
    encoding = environ.get("HGENCODING")
    if not encoding:
        encoding = locale.getpreferredencoding().encode('ascii') or 'ascii'
        encoding = _encodingfixers.get(encoding, lambda: encoding)()
except locale.Error:
    encoding = 'ascii'
encodingmode = environ.get("HGENCODINGMODE", "strict")
fallbackencoding = 'ISO-8859-1'

class localstr(bytes):
    '''This class allows strings that are unmodified to be
    round-tripped to the local encoding and back'''
    def __new__(cls, u, l):
        s = bytes.__new__(cls, l)
        s._utf8 = u
        return s
    def __hash__(self):
        return hash(self._utf8) # avoid collisions in local string space

def tolocal(s):
    """
    Convert a string from internal UTF-8 to local encoding

    All internal strings should be UTF-8 but some repos before the
    implementation of locale support may contain latin1 or possibly
    other character sets. We attempt to decode everything strictly
    using UTF-8, then Latin-1, and failing that, we use UTF-8 and
    replace unknown characters.

    The localstr class is used to cache the known UTF-8 encoding of
    strings next to their local representation to allow lossless
    round-trip conversion back to UTF-8.

    >>> u = b'foo: \\xc3\\xa4' # utf-8
    >>> l = tolocal(u)
    >>> l
    'foo: ?'
    >>> fromlocal(l)
    'foo: \\xc3\\xa4'
    >>> u2 = b'foo: \\xc3\\xa1'
    >>> d = { l: 1, tolocal(u2): 2 }
    >>> len(d) # no collision
    2
    >>> b'foo: ?' in d
    False
    >>> l1 = b'foo: \\xe4' # historical latin1 fallback
    >>> l = tolocal(l1)
    >>> l
    'foo: ?'
    >>> fromlocal(l) # magically in utf-8
    'foo: \\xc3\\xa4'
    """

    if isasciistr(s):
        return s

    try:
        try:
            # make sure string is actually stored in UTF-8
            u = s.decode('UTF-8')
            if encoding == 'UTF-8':
                # fast path
                return s
            r = u.encode(_sysstr(encoding), u"replace")
            if u == r.decode(_sysstr(encoding)):
                # r is a safe, non-lossy encoding of s
                return r
            return localstr(s, r)
        except UnicodeDecodeError:
            # we should only get here if we're looking at an ancient changeset
            try:
                u = s.decode(_sysstr(fallbackencoding))
                r = u.encode(_sysstr(encoding), u"replace")
                if u == r.decode(_sysstr(encoding)):
                    # r is a safe, non-lossy encoding of s
                    return r
                return localstr(u.encode('UTF-8'), r)
            except UnicodeDecodeError:
                u = s.decode("utf-8", "replace") # last ditch
                # can't round-trip
                return u.encode(_sysstr(encoding), u"replace")
    except LookupError as k:
        raise error.Abort(k, hint="please check your locale settings")

def fromlocal(s):
    """
    Convert a string from the local character encoding to UTF-8

    We attempt to decode strings using the encoding mode set by
    HGENCODINGMODE, which defaults to 'strict'. In this mode, unknown
    characters will cause an error message. Other modes include
    'replace', which replaces unknown characters with a special
    Unicode character, and 'ignore', which drops the character.
    """

    # can we do a lossless round-trip?
    if isinstance(s, localstr):
        return s._utf8
    if isasciistr(s):
        return s

    try:
        u = s.decode(_sysstr(encoding), _sysstr(encodingmode))
        return u.encode("utf-8")
    except UnicodeDecodeError as inst:
        sub = s[max(0, inst.start - 10):inst.start + 10]
        raise error.Abort("decoding near '%s': %s!" % (sub, inst))
    except LookupError as k:
        raise error.Abort(k, hint="please check your locale settings")

def unitolocal(u):
    """Convert a unicode string to a byte string of local encoding"""
    return tolocal(u.encode('utf-8'))

def unifromlocal(s):
    """Convert a byte string of local encoding to a unicode string"""
    return fromlocal(s).decode('utf-8')

def unimethod(bytesfunc):
    """Create a proxy method that forwards __unicode__() and __str__() of
    Python 3 to __bytes__()"""
    def unifunc(obj):
        return unifromlocal(bytesfunc(obj))
    return unifunc

# converter functions between native str and byte string. use these if the
# character encoding is not aware (e.g. exception message) or is known to
# be locale dependent (e.g. date formatting.)
if pycompat.ispy3:
    strtolocal = unitolocal
    strfromlocal = unifromlocal
    strmethod = unimethod
else:
    strtolocal = pycompat.identity
    strfromlocal = pycompat.identity
    strmethod = pycompat.identity

if not _nativeenviron:
    # now encoding and helper functions are available, recreate the environ
    # dict to be exported to other modules
    environ = dict((tolocal(k.encode(u'utf-8')), tolocal(v.encode(u'utf-8')))
                   for k, v in os.environ.items())  # re-exports

# How to treat ambiguous-width characters. Set to 'wide' to treat as wide.
_wide = _sysstr(environ.get("HGENCODINGAMBIGUOUS", "narrow") == "wide"
                and "WFA" or "WF")

def colwidth(s):
    "Find the column width of a string for display in the local encoding"
    return ucolwidth(s.decode(_sysstr(encoding), u'replace'))

def ucolwidth(d):
    "Find the column width of a Unicode string for display"
    eaw = getattr(unicodedata, 'east_asian_width', None)
    if eaw is not None:
        return sum([eaw(c) in _wide and 2 or 1 for c in d])
    return len(d)

def getcols(s, start, c):
    '''Use colwidth to find a c-column substring of s starting at byte
    index start'''
    for x in xrange(start + c, len(s)):
        t = s[start:x]
        if colwidth(t) == c:
            return t

def trim(s, width, ellipsis='', leftside=False):
    """Trim string 's' to at most 'width' columns (including 'ellipsis').

    If 'leftside' is True, left side of string 's' is trimmed.
    'ellipsis' is always placed at trimmed side.

    >>> from .node import bin
    >>> def bprint(s):
    ...     print(pycompat.sysstr(s))
    >>> ellipsis = b'+++'
    >>> from . import encoding
    >>> encoding.encoding = b'utf-8'
    >>> t = b'1234567890'
    >>> bprint(trim(t, 12, ellipsis=ellipsis))
    1234567890
    >>> bprint(trim(t, 10, ellipsis=ellipsis))
    1234567890
    >>> bprint(trim(t, 8, ellipsis=ellipsis))
    12345+++
    >>> bprint(trim(t, 8, ellipsis=ellipsis, leftside=True))
    +++67890
    >>> bprint(trim(t, 8))
    12345678
    >>> bprint(trim(t, 8, leftside=True))
    34567890
    >>> bprint(trim(t, 3, ellipsis=ellipsis))
    +++
    >>> bprint(trim(t, 1, ellipsis=ellipsis))
    +
    >>> u = u'\u3042\u3044\u3046\u3048\u304a' # 2 x 5 = 10 columns
    >>> t = u.encode(pycompat.sysstr(encoding.encoding))
    >>> bprint(trim(t, 12, ellipsis=ellipsis))
    \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a
    >>> bprint(trim(t, 10, ellipsis=ellipsis))
    \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a
    >>> bprint(trim(t, 8, ellipsis=ellipsis))
    \xe3\x81\x82\xe3\x81\x84+++
    >>> bprint(trim(t, 8, ellipsis=ellipsis, leftside=True))
    +++\xe3\x81\x88\xe3\x81\x8a
    >>> bprint(trim(t, 5))
    \xe3\x81\x82\xe3\x81\x84
    >>> bprint(trim(t, 5, leftside=True))
    \xe3\x81\x88\xe3\x81\x8a
    >>> bprint(trim(t, 4, ellipsis=ellipsis))
    +++
    >>> bprint(trim(t, 4, ellipsis=ellipsis, leftside=True))
    +++
    >>> t = bin(b'112233445566778899aa') # invalid byte sequence
    >>> bprint(trim(t, 12, ellipsis=ellipsis))
    \x11\x22\x33\x44\x55\x66\x77\x88\x99\xaa
    >>> bprint(trim(t, 10, ellipsis=ellipsis))
    \x11\x22\x33\x44\x55\x66\x77\x88\x99\xaa
    >>> bprint(trim(t, 8, ellipsis=ellipsis))
    \x11\x22\x33\x44\x55+++
    >>> bprint(trim(t, 8, ellipsis=ellipsis, leftside=True))
    +++\x66\x77\x88\x99\xaa
    >>> bprint(trim(t, 8))
    \x11\x22\x33\x44\x55\x66\x77\x88
    >>> bprint(trim(t, 8, leftside=True))
    \x33\x44\x55\x66\x77\x88\x99\xaa
    >>> bprint(trim(t, 3, ellipsis=ellipsis))
    +++
    >>> bprint(trim(t, 1, ellipsis=ellipsis))
    +
    """
    try:
        u = s.decode(_sysstr(encoding))
    except UnicodeDecodeError:
        if len(s) <= width: # trimming is not needed
            return s
        width -= len(ellipsis)
        if width <= 0: # no enough room even for ellipsis
            return ellipsis[:width + len(ellipsis)]
        if leftside:
            return ellipsis + s[-width:]
        return s[:width] + ellipsis

    if ucolwidth(u) <= width: # trimming is not needed
        return s

    width -= len(ellipsis)
    if width <= 0: # no enough room even for ellipsis
        return ellipsis[:width + len(ellipsis)]

    if leftside:
        uslice = lambda i: u[i:]
        concat = lambda s: ellipsis + s
    else:
        uslice = lambda i: u[:-i]
        concat = lambda s: s + ellipsis
    for i in xrange(1, len(u)):
        usub = uslice(i)
        if ucolwidth(usub) <= width:
            return concat(usub.encode(_sysstr(encoding)))
    return ellipsis # no enough room for multi-column characters

def lower(s):
    "best-effort encoding-aware case-folding of local string s"
    try:
        return asciilower(s)
    except UnicodeDecodeError:
        pass
    try:
        if isinstance(s, localstr):
            u = s._utf8.decode("utf-8")
        else:
            u = s.decode(_sysstr(encoding), _sysstr(encodingmode))

        lu = u.lower()
        if u == lu:
            return s # preserve localstring
        return lu.encode(_sysstr(encoding))
    except UnicodeError:
        return s.lower() # we don't know how to fold this except in ASCII
    except LookupError as k:
        raise error.Abort(k, hint="please check your locale settings")

def upper(s):
    "best-effort encoding-aware case-folding of local string s"
    try:
        return asciiupper(s)
    except UnicodeDecodeError:
        return upperfallback(s)

def upperfallback(s):
    try:
        if isinstance(s, localstr):
            u = s._utf8.decode("utf-8")
        else:
            u = s.decode(_sysstr(encoding), _sysstr(encodingmode))

        uu = u.upper()
        if u == uu:
            return s # preserve localstring
        return uu.encode(_sysstr(encoding))
    except UnicodeError:
        return s.upper() # we don't know how to fold this except in ASCII
    except LookupError as k:
        raise error.Abort(k, hint="please check your locale settings")

class normcasespecs(object):
    '''what a platform's normcase does to ASCII strings

    This is specified per platform, and should be consistent with what normcase
    on that platform actually does.

    lower: normcase lowercases ASCII strings
    upper: normcase uppercases ASCII strings
    other: the fallback function should always be called

    This should be kept in sync with normcase_spec in util.h.'''
    lower = -1
    upper = 1
    other = 0

def jsonescape(s, paranoid=False):
    '''returns a string suitable for JSON

    JSON is problematic for us because it doesn't support non-Unicode
    bytes. To deal with this, we take the following approach:

    - localstr objects are converted back to UTF-8
    - valid UTF-8/ASCII strings are passed as-is
    - other strings are converted to UTF-8b surrogate encoding
    - apply JSON-specified string escaping

    (escapes are doubled in these tests)

    >>> jsonescape(b'this is a test')
    'this is a test'
    >>> jsonescape(b'escape characters: \\0 \\x0b \\x7f')
    'escape characters: \\\\u0000 \\\\u000b \\\\u007f'
    >>> jsonescape(b'escape characters: \\b \\t \\n \\f \\r \\" \\\\')
    'escape characters: \\\\b \\\\t \\\\n \\\\f \\\\r \\\\" \\\\\\\\'
    >>> jsonescape(b'a weird byte: \\xdd')
    'a weird byte: \\xed\\xb3\\x9d'
    >>> jsonescape(b'utf-8: caf\\xc3\\xa9')
    'utf-8: caf\\xc3\\xa9'
    >>> jsonescape(b'')
    ''

    If paranoid, non-ascii and common troublesome characters are also escaped.
    This is suitable for web output.

    >>> s = b'escape characters: \\0 \\x0b \\x7f'
    >>> assert jsonescape(s) == jsonescape(s, paranoid=True)
    >>> s = b'escape characters: \\b \\t \\n \\f \\r \\" \\\\'
    >>> assert jsonescape(s) == jsonescape(s, paranoid=True)
    >>> jsonescape(b'escape boundary: \\x7e \\x7f \\xc2\\x80', paranoid=True)
    'escape boundary: ~ \\\\u007f \\\\u0080'
    >>> jsonescape(b'a weird byte: \\xdd', paranoid=True)
    'a weird byte: \\\\udcdd'
    >>> jsonescape(b'utf-8: caf\\xc3\\xa9', paranoid=True)
    'utf-8: caf\\\\u00e9'
    >>> jsonescape(b'non-BMP: \\xf0\\x9d\\x84\\x9e', paranoid=True)
    'non-BMP: \\\\ud834\\\\udd1e'
    >>> jsonescape(b'<foo@example.org>', paranoid=True)
    '\\\\u003cfoo@example.org\\\\u003e'
    '''

    u8chars = toutf8b(s)
    try:
        return _jsonescapeu8fast(u8chars, paranoid)
    except ValueError:
        pass
    return charencodepure.jsonescapeu8fallback(u8chars, paranoid)

# We need to decode/encode U+DCxx codes transparently since invalid UTF-8
# bytes are mapped to that range.
if pycompat.ispy3:
    _utf8strict = r'surrogatepass'
else:
    _utf8strict = r'strict'

_utf8len = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 4]

def getutf8char(s, pos):
    '''get the next full utf-8 character in the given string, starting at pos

    Raises a UnicodeError if the given location does not start a valid
    utf-8 character.
    '''

    # find how many bytes to attempt decoding from first nibble
    l = _utf8len[ord(s[pos:pos + 1]) >> 4]
    if not l: # ascii
        return s[pos:pos + 1]

    c = s[pos:pos + l]
    # validate with attempted decode
    c.decode("utf-8", _utf8strict)
    return c

def toutf8b(s):
    '''convert a local, possibly-binary string into UTF-8b

    This is intended as a generic method to preserve data when working
    with schemes like JSON and XML that have no provision for
    arbitrary byte strings. As Mercurial often doesn't know
    what encoding data is in, we use so-called UTF-8b.

    If a string is already valid UTF-8 (or ASCII), it passes unmodified.
    Otherwise, unsupported bytes are mapped to UTF-16 surrogate range,
    uDC00-uDCFF.

    Principles of operation:

    - ASCII and UTF-8 data successfully round-trips and is understood
      by Unicode-oriented clients
    - filenames and file contents in arbitrary other encodings can have
      be round-tripped or recovered by clueful clients
    - local strings that have a cached known UTF-8 encoding (aka
      localstr) get sent as UTF-8 so Unicode-oriented clients get the
      Unicode data they want
    - because we must preserve UTF-8 bytestring in places such as
      filenames, metadata can't be roundtripped without help

    (Note: "UTF-8b" often refers to decoding a mix of valid UTF-8 and
    arbitrary bytes into an internal Unicode format that can be
    re-encoded back into the original. Here we are exposing the
    internal surrogate encoding as a UTF-8 string.)
    '''

    if not isinstance(s, localstr) and isasciistr(s):
        return s
    if "\xed" not in s:
        if isinstance(s, localstr):
            return s._utf8
        try:
            s.decode('utf-8', _utf8strict)
            return s
        except UnicodeDecodeError:
            pass

    s = pycompat.bytestr(s)
    r = ""
    pos = 0
    l = len(s)
    while pos < l:
        try:
            c = getutf8char(s, pos)
            if "\xed\xb0\x80" <= c <= "\xed\xb3\xbf":
                # have to re-escape existing U+DCxx characters
                c = unichr(0xdc00 + ord(s[pos])).encode('utf-8', _utf8strict)
                pos += 1
            else:
                pos += len(c)
        except UnicodeDecodeError:
            c = unichr(0xdc00 + ord(s[pos])).encode('utf-8', _utf8strict)
            pos += 1
        r += c
    return r

def fromutf8b(s):
    '''Given a UTF-8b string, return a local, possibly-binary string.

    return the original binary string. This
    is a round-trip process for strings like filenames, but metadata
    that's was passed through tolocal will remain in UTF-8.

    >>> roundtrip = lambda x: fromutf8b(toutf8b(x)) == x
    >>> m = b"\\xc3\\xa9\\x99abcd"
    >>> toutf8b(m)
    '\\xc3\\xa9\\xed\\xb2\\x99abcd'
    >>> roundtrip(m)
    True
    >>> roundtrip(b"\\xc2\\xc2\\x80")
    True
    >>> roundtrip(b"\\xef\\xbf\\xbd")
    True
    >>> roundtrip(b"\\xef\\xef\\xbf\\xbd")
    True
    >>> roundtrip(b"\\xf1\\x80\\x80\\x80\\x80")
    True
    '''

    if isasciistr(s):
        return s
    # fast path - look for uDxxx prefixes in s
    if "\xed" not in s:
        return s

    # We could do this with the unicode type but some Python builds
    # use UTF-16 internally (issue5031) which causes non-BMP code
    # points to be escaped. Instead, we use our handy getutf8char
    # helper again to walk the string without "decoding" it.

    s = pycompat.bytestr(s)
    r = ""
    pos = 0
    l = len(s)
    while pos < l:
        c = getutf8char(s, pos)
        pos += len(c)
        # unescape U+DCxx characters
        if "\xed\xb0\x80" <= c <= "\xed\xb3\xbf":
            c = pycompat.bytechr(ord(c.decode("utf-8", _utf8strict)) & 0xff)
        r += c
    return r

if pycompat.ispy3:
    class strio(io.TextIOWrapper):
        """Wrapper around TextIOWrapper that respects hg's encoding assumptions.

        Also works around Python closing streams.
        """

        def __init__(self, buffer):
            super(strio, self).__init__(buffer, encoding=_sysstr(encoding))

        def __del__(self):
            """Override __del__ so it doesn't close the underlying stream."""
else:
    strio = pycompat.identity
