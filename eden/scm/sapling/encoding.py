# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# encoding.py - character transcoding support for Mercurial
#
#  Copyright 2005-2009 Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import locale
import os
import sys
import unicodedata

import bindings

from .pure import charencode as charencodepure

charencode = bindings.cext.parsers

isasciistr = charencode.isasciistr
asciilower = charencode.asciilower
asciiupper = charencode.asciiupper
_jsonescapeu8fast = charencode.jsonescapeu8fast

# These unicode characters are ignored by HFS+ (Apple Technote 1150,
# "Unicode Subtleties"), so we need to ignore them in some places for
# sanity.
_ignore = [
    chr(int(x, 16)).encode("utf-8")
    for x in (
        "200c 200d 200e 200f 202a 202b 202c 202d 202e "
        "206a 206b 206c 206d 206e 206f feff"
    ).split()
]
# verify the next function will work
assert all(i.startswith((b"\xe2", b"\xef")) for i in _ignore)


def hfsignoreclean(s):
    """Remove codepoints ignored by HFS+ from s.

    >>> hfsignoreclean(u'.h\u200cg'.encode('utf-8'))
    b'.hg'
    >>> hfsignoreclean(u'.h\ufeffg'.encode('utf-8'))
    b'.hg'
    """

    if isinstance(s, bytes):
        if b"\xe2" in s or b"\xef" in s:
            for c in _ignore:
                s = s.replace(c, b"")
    elif isinstance(s, str):
        # It's unfortunate that we encode every string, probably resulting in an
        # allocation, but it saves us from having to iterate over the string for
        # every ignored code point.
        bytestr = s.encode("utf-8")
        if b"\xe2" in bytestr or b"\xef" in bytestr:
            for c in _ignore:
                bytestr = bytestr.replace(c, b"")
            s = bytestr.decode("utf-8")
    else:
        raise RuntimeError("cannot scrub hfs path of type %s" % s.__class__)
    return s


def setfromenviron():
    """Reset encoding states from environment variables"""
    global encoding, outputencoding, encodingmode, environ, _wide
    environ = os.environ  # re-exports
    try:
        encoding = os.environ.get("HGENCODING")
        if not encoding:
            encoding = locale.getpreferredencoding() or "ascii"
            encoding = _encodingfixers.get(encoding, lambda: encoding)()
    except locale.Error:
        encoding = "ascii"
    encodingmode = os.environ.get("HGENCODINGMODE", "strict")

    if encoding == "ascii":
        encoding = "utf-8"

    # How to treat ambiguous-width characters. Set to 'wide' to treat as wide.
    _wide = os.environ.get("HGENCODINGAMBIGUOUS", "narrow") == "wide" and "WFA" or "WF"

    outputencoding = os.environ.get("HGOUTPUTENCODING")

    if outputencoding == "ascii":
        outputencoding = "utf-8"

    # On Windows the outputencoding will be set to the OEM code page by the
    # windows module when it is loaded.


_encodingfixers = {"646": lambda: "ascii", "ANSI_X3.4-1968": lambda: "ascii"}

# No idea if it should be rewritten to the canonical name 'utf-8' on Python 3.
# https://bugs.python.org/issue13216

environ = encoding = outputencoding = encodingmode = _wide = None
setfromenviron()
fallbackencoding = "ISO-8859-1"


def _setascii():
    """Set encoding to ascii. Used by some doctests."""
    global encoding
    encoding = "ascii"


def _colwidth(s):
    "Find the column width of a string for display in the local encoding"
    return ucolwidth(s.decode(encoding, "replace"))


def ucolwidth(d):
    "Find the column width of a Unicode string for display"
    eaw = getattr(unicodedata, "east_asian_width", None)
    if eaw is not None:
        return sum([eaw(c) in _wide and 2 or 1 for c in d])
    return len(d)


def getcols(s, start, c):
    """Use colwidth to find a c-column substring of s starting at byte
    index start"""
    for x in range(start + c, start, -1):
        t = s[start:x]
        if colwidth(t) == c:
            return t


def trim(s, width, ellipsis="", leftside=False):
    """Trim string 's' to at most 'width' columns (including 'ellipsis').

    If 'leftside' is True, left side of string 's' is trimmed.
    'ellipsis' is always placed at trimmed side.

    >>> from .node import bin
    >>> ellipsis = '+++'
    >>> from . import encoding
    >>> encoding.encoding = 'utf-8'
    >>> t = '1234567890'
    >>> print(trim(t, 12, ellipsis=ellipsis))
    1234567890
    >>> print(trim(t, 10, ellipsis=ellipsis))
    1234567890
    >>> print(trim(t, 8, ellipsis=ellipsis))
    12345+++
    >>> print(trim(t, 8, ellipsis=ellipsis, leftside=True))
    +++67890
    >>> print(trim(t, 8))
    12345678
    >>> print(trim(t, 8, leftside=True))
    34567890
    >>> print(trim(t, 3, ellipsis=ellipsis))
    +++
    >>> print(trim(t, 1, ellipsis=ellipsis))
    +
    >>> t = u'\u3042\u3044\u3046\u3048\u304a' # 2 x 5 = 10 columns
    >>> print(trim(t, 12, ellipsis=ellipsis))
    あいうえお
    >>> print(trim(t, 10, ellipsis=ellipsis))
    あいうえお
    >>> print(trim(t, 8, ellipsis=ellipsis))
    あい+++
    >>> print(trim(t, 8, ellipsis=ellipsis, leftside=True))
    +++えお
    >>> print(trim(t, 5))
    あい
    >>> print(trim(t, 5, leftside=True))
    えお
    >>> print(trim(t, 4, ellipsis=ellipsis))
    +++
    >>> print(trim(t, 4, ellipsis=ellipsis, leftside=True))
    +++
    """
    try:
        if sys.version_info.major == 3:
            u = s
        else:
            u = s.decode(encoding)
    except UnicodeDecodeError:
        if len(s) <= width:  # trimming is not needed
            return s
        width -= len(ellipsis)
        if width <= 0:  # no enough room even for ellipsis
            return ellipsis[: width + len(ellipsis)]
        if leftside:
            return ellipsis + s[-width:]
        return s[:width] + ellipsis

    if ucolwidth(u) <= width:  # trimming is not needed
        return s

    width -= len(ellipsis)
    if width <= 0:  # no enough room even for ellipsis
        return ellipsis[: width + len(ellipsis)]

    if leftside:
        uslice = lambda i: u[i:]
        concat = lambda s: ellipsis + s
    else:
        uslice = lambda i: u[:-i]
        concat = lambda s: s + ellipsis
    for i in range(1, len(u)):
        usub = uslice(i)
        if ucolwidth(usub) <= width:
            if sys.version_info[0] == 3:
                return concat(usub)
            else:
                return concat(usub.encode(encoding))
    return ellipsis  # no enough room for multi-column characters


def upperfallback(s):
    return s.upper()


class normcasespecs:
    """what a platform's normcase does to ASCII strings

    This is specified per platform, and should be consistent with what normcase
    on that platform actually does.

    lower: normcase lowercases ASCII strings
    upper: normcase uppercases ASCII strings
    other: the fallback function should always be called

    This should be kept in sync with normcase_spec in util.h."""

    lower = -1
    upper = 1
    other = 0


def jsonescape(s, paranoid=False):
    r"""returns a string suitable for JSON

    JSON is problematic for us because it doesn't support non-Unicode
    bytes. To deal with this, we take the following approach:

    - valid UTF-8/ASCII strings are passed as-is
    - other strings are converted to UTF-8b surrogate encoding
    - apply JSON-specified string escaping

    (escapes are doubled in these tests)

    >>> jsonescape(b'this is a test')
    b'this is a test'
    >>> jsonescape(b'escape characters: \\0 \\x0b \\x7f')
    b'escape characters: \\\\0 \\\\x0b \\\\x7f'
    >>> jsonescape(b'escape characters: \\b \\t \\n \\f \\r \\" \\\\')
    b'escape characters: \\\\b \\\\t \\\\n \\\\f \\\\r \\\\\\" \\\\\\\\'
    >>> jsonescape(b'a weird byte: \\xdd')
    b'a weird byte: \\\\xdd'
    >>> jsonescape(b'utf-8: calf\\xc3\\xa9')
    b'utf-8: calf\\\\xc3\\\\xa9'
    >>> jsonescape(b'')
    b''

    If paranoid, non-ascii and common troublesome characters are also escaped.
    This is suitable for web output.

    >>> s = b'escape characters: \\0 \\x0b \\x7f'
    >>> assert jsonescape(s) == jsonescape(s, paranoid=True)
    >>> s = b'escape characters: \\b \\t \\n \\f \\r \\" \\\\'
    >>> assert jsonescape(s) == jsonescape(s, paranoid=True)
    >>> jsonescape(b'escape boundary: \\x7e \\x7f \\xc2\\x80', paranoid=True)
    b'escape boundary: \\\\x7e \\\\x7f \\\\xc2\\\\x80'
    >>> jsonescape(b'a weird byte: \\xdd', paranoid=True)
    b'a weird byte: \\\\xdd'
    >>> jsonescape(b'utf-8: calf\\xc3\\xa9', paranoid=True)
    b'utf-8: calf\\\\xc3\\\\xa9'
    >>> jsonescape(b'non-BMP: \\xf0\\x9d\\x84\\x9e', paranoid=True)
    b'non-BMP: \\\\xf0\\\\x9d\\\\x84\\\\x9e'
    >>> jsonescape(b'<foo@example.org>', paranoid=True)
    b'\\u003cfoo@example.org\\u003e'
    """

    u8chars = toutf8b(s)
    try:
        return _jsonescapeu8fast(u8chars, paranoid)
    except ValueError:
        pass
    return charencodepure.jsonescapeu8fallback(u8chars, paranoid)


# We need to decode/encode U+DCxx codes transparently since invalid UTF-8
# bytes are mapped to that range.

_utf8strict = r"surrogatepass"

_utf8len = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 4]


def getutf8char(s, pos):
    """get the next full utf-8 character in the given string, starting at pos

    Raises a UnicodeError if the given location does not start a valid
    utf-8 character.
    """

    # find how many bytes to attempt decoding from first nibble
    l = _utf8len[ord(s[pos : pos + 1]) >> 4]
    if not l:  # ascii
        return s[pos : pos + 1]

    c = s[pos : pos + l]
    # validate with attempted decode
    c.decode("utf-8", _utf8strict)
    return c


def toutf8b(s):
    """convert a local, possibly-binary string into UTF-8b

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
    - because we must preserve UTF-8 bytestring in places such as
      filenames, metadata can't be roundtripped without help

    (Note: "UTF-8b" often refers to decoding a mix of valid UTF-8 and
    arbitrary bytes into an internal Unicode format that can be
    re-encoded back into the original. Here we are exposing the
    internal surrogate encoding as a UTF-8 string.)
    """

    if isasciistr(s):
        return s
    if b"\xed" not in s:
        try:
            s.decode("utf-8", _utf8strict)
            return s
        except UnicodeDecodeError:
            pass

    r = b""
    pos = 0
    l = len(s)
    while pos < l:
        try:
            c = getutf8char(s, pos)
            if b"\xed\xb0\x80" <= c <= b"\xed\xb3\xbf":
                # have to re-escape existing U+DCxx characters
                value = s[pos]
                c = chr(0xDC00 + value).encode("utf-8", _utf8strict)
                pos += 1
            else:
                pos += len(c)
        except UnicodeDecodeError:
            value = s[pos]
            c = chr(0xDC00 + value).encode("utf-8", _utf8strict)
            pos += 1
        r += c
    return r


# Prefer native unicode on Python
colwidth = ucolwidth
