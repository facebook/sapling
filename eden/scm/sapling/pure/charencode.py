# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# charencode.py - miscellaneous character encoding
#
#  Copyright 2005-2009 Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import array


def isasciistr(s):
    try:
        s.decode("ascii")
        return True
    except UnicodeDecodeError:
        return False


def asciilower(s):
    """convert a string to lowercase if ASCII

    Raises UnicodeDecodeError if non-ASCII characters are found."""
    s.decode("ascii")
    return s.lower()


def asciiupper(s):
    """convert a string to uppercase if ASCII

    Raises UnicodeDecodeError if non-ASCII characters are found."""
    s.decode("ascii")
    return s.upper()


_jsonmap = []
_jsonmap.extend("\\u%04x" % x for x in range(32))
_jsonmap.extend(chr(x) for x in range(32, 127))
_jsonmap.append("\\u007f")
_jsonmap[0x09] = "\\t"
_jsonmap[0x0A] = "\\n"
_jsonmap[0x22] = '\\"'
_jsonmap[0x5C] = "\\\\"
_jsonmap[0x08] = "\\b"
_jsonmap[0x0C] = "\\f"
_jsonmap[0x0D] = "\\r"
_paranoidjsonmap = _jsonmap[:]
_paranoidjsonmap[0x3C] = "\\u003c"  # '<' (e.g. escape "</script>")
_paranoidjsonmap[0x3E] = "\\u003e"  # '>'
_jsonmap.extend(chr(x) for x in range(128, 256))


def jsonescapeu8fast(u8chars, paranoid):
    """Convert a UTF-8 byte string to JSON-escaped form (fast path)

    Raises ValueError if non-ASCII characters have to be escaped.
    """
    if paranoid:
        jm = _paranoidjsonmap
    else:
        jm = _jsonmap
    try:
        return "".join(jm[x] for x in bytearray(u8chars))
    except IndexError:
        raise ValueError


_utf8strict = r"surrogatepass"


def jsonescapeu8fallback(u8chars, paranoid):
    """Convert a UTF-8 byte string to JSON-escaped form (slow path)

    Escapes all non-ASCII characters no matter if paranoid is False.
    """
    if paranoid:
        jm = _paranoidjsonmap
    else:
        jm = _jsonmap
    # non-BMP char is represented as UTF-16 surrogate pair
    u16b = u8chars.decode("utf-8", _utf8strict).encode("utf-16", _utf8strict)
    u16codes = array.array(r"H", u16b)
    u16codes.pop(0)  # drop BOM
    return "".join(jm[x] if x < 128 else "\\u%04x" % x for x in u16codes)
