# encoding.py - character transcoding support for Mercurial
#
#  Copyright 2005-2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import error
import unicodedata, locale, os

# These unicode characters are ignored by HFS+ (Apple Technote 1150,
# "Unicode Subtleties"), so we need to ignore them in some places for
# sanity.
_ignore = [unichr(int(x, 16)).encode("utf-8") for x in
           "200c 200d 200e 200f 202a 202b 202c 202d 202e "
           "206a 206b 206c 206d 206e 206f feff".split()]
# verify the next function will work
assert set([i[0] for i in _ignore]) == set(["\xe2", "\xef"])

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

def _getpreferredencoding():
    '''
    On darwin, getpreferredencoding ignores the locale environment and
    always returns mac-roman. http://bugs.python.org/issue6202 fixes this
    for Python 2.7 and up. This is the same corrected code for earlier
    Python versions.

    However, we can't use a version check for this method, as some distributions
    patch Python to fix this. Instead, we use it as a 'fixer' for the mac-roman
    encoding, as it is unlikely that this encoding is the actually expected.
    '''
    try:
        locale.CODESET
    except AttributeError:
        # Fall back to parsing environment variables :-(
        return locale.getdefaultlocale()[1]

    oldloc = locale.setlocale(locale.LC_CTYPE)
    locale.setlocale(locale.LC_CTYPE, "")
    result = locale.nl_langinfo(locale.CODESET)
    locale.setlocale(locale.LC_CTYPE, oldloc)

    return result

_encodingfixers = {
    '646': lambda: 'ascii',
    'ANSI_X3.4-1968': lambda: 'ascii',
    'mac-roman': _getpreferredencoding
}

try:
    encoding = os.environ.get("HGENCODING")
    if not encoding:
        encoding = locale.getpreferredencoding() or 'ascii'
        encoding = _encodingfixers.get(encoding, lambda: encoding)()
except locale.Error:
    encoding = 'ascii'
encodingmode = os.environ.get("HGENCODINGMODE", "strict")
fallbackencoding = 'ISO-8859-1'

class localstr(str):
    '''This class allows strings that are unmodified to be
    round-tripped to the local encoding and back'''
    def __new__(cls, u, l):
        s = str.__new__(cls, l)
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

    >>> u = 'foo: \\xc3\\xa4' # utf-8
    >>> l = tolocal(u)
    >>> l
    'foo: ?'
    >>> fromlocal(l)
    'foo: \\xc3\\xa4'
    >>> u2 = 'foo: \\xc3\\xa1'
    >>> d = { l: 1, tolocal(u2): 2 }
    >>> len(d) # no collision
    2
    >>> 'foo: ?' in d
    False
    >>> l1 = 'foo: \\xe4' # historical latin1 fallback
    >>> l = tolocal(l1)
    >>> l
    'foo: ?'
    >>> fromlocal(l) # magically in utf-8
    'foo: \\xc3\\xa4'
    """

    try:
        try:
            # make sure string is actually stored in UTF-8
            u = s.decode('UTF-8')
            if encoding == 'UTF-8':
                # fast path
                return s
            r = u.encode(encoding, "replace")
            if u == r.decode(encoding):
                # r is a safe, non-lossy encoding of s
                return r
            return localstr(s, r)
        except UnicodeDecodeError:
            # we should only get here if we're looking at an ancient changeset
            try:
                u = s.decode(fallbackencoding)
                r = u.encode(encoding, "replace")
                if u == r.decode(encoding):
                    # r is a safe, non-lossy encoding of s
                    return r
                return localstr(u.encode('UTF-8'), r)
            except UnicodeDecodeError:
                u = s.decode("utf-8", "replace") # last ditch
                return u.encode(encoding, "replace") # can't round-trip
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

    try:
        return s.decode(encoding, encodingmode).encode("utf-8")
    except UnicodeDecodeError as inst:
        sub = s[max(0, inst.start - 10):inst.start + 10]
        raise error.Abort("decoding near '%s': %s!" % (sub, inst))
    except LookupError as k:
        raise error.Abort(k, hint="please check your locale settings")

# How to treat ambiguous-width characters. Set to 'wide' to treat as wide.
wide = (os.environ.get("HGENCODINGAMBIGUOUS", "narrow") == "wide"
        and "WFA" or "WF")

def colwidth(s):
    "Find the column width of a string for display in the local encoding"
    return ucolwidth(s.decode(encoding, 'replace'))

def ucolwidth(d):
    "Find the column width of a Unicode string for display"
    eaw = getattr(unicodedata, 'east_asian_width', None)
    if eaw is not None:
        return sum([eaw(c) in wide and 2 or 1 for c in d])
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

    >>> ellipsis = '+++'
    >>> from mercurial import encoding
    >>> encoding.encoding = 'utf-8'
    >>> t= '1234567890'
    >>> print trim(t, 12, ellipsis=ellipsis)
    1234567890
    >>> print trim(t, 10, ellipsis=ellipsis)
    1234567890
    >>> print trim(t, 8, ellipsis=ellipsis)
    12345+++
    >>> print trim(t, 8, ellipsis=ellipsis, leftside=True)
    +++67890
    >>> print trim(t, 8)
    12345678
    >>> print trim(t, 8, leftside=True)
    34567890
    >>> print trim(t, 3, ellipsis=ellipsis)
    +++
    >>> print trim(t, 1, ellipsis=ellipsis)
    +
    >>> u = u'\u3042\u3044\u3046\u3048\u304a' # 2 x 5 = 10 columns
    >>> t = u.encode(encoding.encoding)
    >>> print trim(t, 12, ellipsis=ellipsis)
    \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a
    >>> print trim(t, 10, ellipsis=ellipsis)
    \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a
    >>> print trim(t, 8, ellipsis=ellipsis)
    \xe3\x81\x82\xe3\x81\x84+++
    >>> print trim(t, 8, ellipsis=ellipsis, leftside=True)
    +++\xe3\x81\x88\xe3\x81\x8a
    >>> print trim(t, 5)
    \xe3\x81\x82\xe3\x81\x84
    >>> print trim(t, 5, leftside=True)
    \xe3\x81\x88\xe3\x81\x8a
    >>> print trim(t, 4, ellipsis=ellipsis)
    +++
    >>> print trim(t, 4, ellipsis=ellipsis, leftside=True)
    +++
    >>> t = '\x11\x22\x33\x44\x55\x66\x77\x88\x99\xaa' # invalid byte sequence
    >>> print trim(t, 12, ellipsis=ellipsis)
    \x11\x22\x33\x44\x55\x66\x77\x88\x99\xaa
    >>> print trim(t, 10, ellipsis=ellipsis)
    \x11\x22\x33\x44\x55\x66\x77\x88\x99\xaa
    >>> print trim(t, 8, ellipsis=ellipsis)
    \x11\x22\x33\x44\x55+++
    >>> print trim(t, 8, ellipsis=ellipsis, leftside=True)
    +++\x66\x77\x88\x99\xaa
    >>> print trim(t, 8)
    \x11\x22\x33\x44\x55\x66\x77\x88
    >>> print trim(t, 8, leftside=True)
    \x33\x44\x55\x66\x77\x88\x99\xaa
    >>> print trim(t, 3, ellipsis=ellipsis)
    +++
    >>> print trim(t, 1, ellipsis=ellipsis)
    +
    """
    try:
        u = s.decode(encoding)
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
            return concat(usub.encode(encoding))
    return ellipsis # no enough room for multi-column characters

def _asciilower(s):
    '''convert a string to lowercase if ASCII

    Raises UnicodeDecodeError if non-ASCII characters are found.'''
    s.decode('ascii')
    return s.lower()

def asciilower(s):
    # delay importing avoids cyclic dependency around "parsers" in
    # pure Python build (util => i18n => encoding => parsers => util)
    import parsers
    impl = getattr(parsers, 'asciilower', _asciilower)
    global asciilower
    asciilower = impl
    return impl(s)

def _asciiupper(s):
    '''convert a string to uppercase if ASCII

    Raises UnicodeDecodeError if non-ASCII characters are found.'''
    s.decode('ascii')
    return s.upper()

def asciiupper(s):
    # delay importing avoids cyclic dependency around "parsers" in
    # pure Python build (util => i18n => encoding => parsers => util)
    import parsers
    impl = getattr(parsers, 'asciiupper', _asciiupper)
    global asciiupper
    asciiupper = impl
    return impl(s)

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
            u = s.decode(encoding, encodingmode)

        lu = u.lower()
        if u == lu:
            return s # preserve localstring
        return lu.encode(encoding)
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
            u = s.decode(encoding, encodingmode)

        uu = u.upper()
        if u == uu:
            return s # preserve localstring
        return uu.encode(encoding)
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

_jsonmap = {}

def jsonescape(s):
    '''returns a string suitable for JSON

    JSON is problematic for us because it doesn't support non-Unicode
    bytes. To deal with this, we take the following approach:

    - localstr objects are converted back to UTF-8
    - valid UTF-8/ASCII strings are passed as-is
    - other strings are converted to UTF-8b surrogate encoding
    - apply JSON-specified string escaping

    (escapes are doubled in these tests)

    >>> jsonescape('this is a test')
    'this is a test'
    >>> jsonescape('escape characters: \\0 \\x0b \\t \\n \\r \\" \\\\')
    'escape characters: \\\\u0000 \\\\u000b \\\\t \\\\n \\\\r \\\\" \\\\\\\\'
    >>> jsonescape('a weird byte: \\xdd')
    'a weird byte: \\xed\\xb3\\x9d'
    >>> jsonescape('utf-8: caf\\xc3\\xa9')
    'utf-8: caf\\xc3\\xa9'
    >>> jsonescape('')
    ''
    '''

    if not _jsonmap:
        for x in xrange(32):
            _jsonmap[chr(x)] = "\u%04x" %x
        for x in xrange(32, 256):
            c = chr(x)
            _jsonmap[c] = c
        _jsonmap['\t'] = '\\t'
        _jsonmap['\n'] = '\\n'
        _jsonmap['\"'] = '\\"'
        _jsonmap['\\'] = '\\\\'
        _jsonmap['\b'] = '\\b'
        _jsonmap['\f'] = '\\f'
        _jsonmap['\r'] = '\\r'

    return ''.join(_jsonmap[c] for c in toutf8b(s))

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

    if isinstance(s, localstr):
        return s._utf8

    try:
        s.decode('utf-8')
        return s
    except UnicodeDecodeError:
        # surrogate-encode any characters that don't round-trip
        s2 = s.decode('utf-8', 'ignore').encode('utf-8')
        r = ""
        pos = 0
        for c in s:
            if s2[pos:pos + 1] == c:
                r += c
                pos += 1
            else:
                r += unichr(0xdc00 + ord(c)).encode('utf-8')
        return r

def fromutf8b(s):
    '''Given a UTF-8b string, return a local, possibly-binary string.

    return the original binary string. This
    is a round-trip process for strings like filenames, but metadata
    that's was passed through tolocal will remain in UTF-8.

    >>> m = "\\xc3\\xa9\\x99abcd"
    >>> n = toutf8b(m)
    >>> n
    '\\xc3\\xa9\\xed\\xb2\\x99abcd'
    >>> fromutf8b(n) == m
    True
    '''

    # fast path - look for uDxxx prefixes in s
    if "\xed" not in s:
        return s

    u = s.decode("utf-8")
    r = ""
    for c in u:
        if ord(c) & 0xff00 == 0xdc00:
            r += chr(ord(c) & 0xff)
        else:
            r += c.encode("utf-8")
    return r
