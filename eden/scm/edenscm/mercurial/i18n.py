# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# i18n.py - internationalization support for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import gettext as gettextmod
import locale
import os
import sys

from . import encoding, identity, pycompat


# modelled after templater.templatepath:
if getattr(sys, "frozen", None) is not None:
    module = pycompat.sysexecutable
else:
    module = pycompat.fsencode(__file__)

try:
    # pyre-fixme[18]: Global name `unicode` is undefined.
    unicode
except NameError:
    unicode = str

_languages = None
if (
    pycompat.iswindows
    # pyre-fixme[16]: Optional type has no attribute `__getitem__`.
    and "LANGUAGE" not in encoding.environ
    and "LC_ALL" not in encoding.environ
    and "LC_MESSAGES" not in encoding.environ
    and "LANG" not in encoding.environ
):
    # Try to detect UI language by "User Interface Language Management" API
    # if no locale variables are set. Note that locale.getdefaultlocale()
    # uses GetLocaleInfo(), which may be different from UI language.
    # (See http://msdn.microsoft.com/en-us/library/dd374098(v=VS.85).aspx )
    try:
        import ctypes

        # pyre-fixme[16]: Module `ctypes` has no attribute `windll`.
        langid = ctypes.windll.kernel32.GetUserDefaultUILanguage()
        _languages = [locale.windows_locale[langid]]
    except (ImportError, AttributeError, KeyError):
        # ctypes not found or unknown langid
        pass

_ugettext = None
_ungettext = None


def setdatapath(datapath):
    datapath = pycompat.fsdecode(datapath)
    localedir = os.path.join(datapath, pycompat.sysstr("locale"))
    t = gettextmod.translation("hg", localedir, _languages, fallback=True)
    global _ugettext
    try:
        _ugettext = t.ugettext
    except AttributeError:
        _ugettext = t.gettext
    global _ungettext
    try:
        _ungettext = t.ungettext
    except AttributeError:
        _ungettext = t.ngettext


_msgcache = {}  # encoding: {message: translation}


def gettext(message):
    """Translate message.

    The message is looked up in the catalog to get a Unicode string,
    which is encoded in the local encoding before being returned.

    Important: message is restricted to characters in the encoding
    given by sys.getdefaultencoding() which is most likely 'ascii'.
    """
    # If message is None, t.ugettext will return u'None' as the
    # translation whereas our callers expect us to return None.
    if message is None or not _ugettext:
        return identity.replace(message)

    cache = _msgcache.setdefault(encoding.encoding, {})
    if message not in cache:
        if type(message) is unicode:
            # goofy unicode docstrings in test
            paragraphs = message.split(u"\n\n")
        else:
            paragraphs = [p.decode("ascii") for p in message.split("\n\n")]
        # Be careful not to translate the empty string -- it holds the
        # meta data of the .po file.
        u = u"\n\n".join([p and _ugettext(p) or u"" for p in paragraphs])
        try:
            # encoding.tolocal cannot be used since it will first try to
            # decode the Unicode string. Calling u.decode(enc) really
            # means u.encode(sys.getdefaultencoding()).decode(enc). Since
            # the Python encoding defaults to 'ascii', this fails if the
            # translated string use non-ASCII characters.
            encodingstr = pycompat.sysstr(encoding.encoding)
            cache[message] = identity.replace(u.encode(encodingstr, "replace"))
        except LookupError:
            # An unknown encoding results in a LookupError.
            cache[message] = identity.replace(message)
    return cache[message]


def ngettext(singular, plural, count):
    """Translate pluralized message.

    The message is looked up in the catalog to get a Unicode string, pluralized
    appropriately based on the value of count.  The Unicode string is encoded
    in the local encoding before being returned.

    Important: singular and plural are restricted to characters in the encoding
    given by sys.getdefaultencoding() which is most likely 'ascii'.
    """
    # If singular or plural are None, t.ugettext will return u'None' as the
    # translation whereas our callers expect us to return None.
    if singular is None or plural is None or not _ungettext:
        return identity.replace(singular if count == 1 else plural)

    # Don't cache pluralized messages.  They are relatively rare, and the
    # content depends on the count.
    translated = _ungettext(singular, plural, count)
    encodingstr = pycompat.sysstr(encoding.encoding)
    return identity.replace(translated.encode(encodingstr, "replace"))


_plain = True


def _getplain():
    if "HGPLAIN" not in encoding.environ and "HGPLAINEXCEPT" not in encoding.environ:
        return False
    exceptions = encoding.environ.get("HGPLAINEXCEPT", "").strip().split(",")
    return "i18n" not in exceptions


def _(message):
    if _plain:
        return identity.replace(message)
    else:
        return gettext(message)


def _n(singular, plural, count):
    if _plain:
        return identity.replace(singular if count == 1 else plural)
    else:
        return ngettext(singular, plural, count)


def limititems(items, maxitems=5):
    if len(items) > maxitems > 0:
        return items[0:maxitems] + ["...and %d more" % (len(items) - maxitems)]
    else:
        return items


def init():
    """inline _plain() so it's faster. called by dispatch._dispatch"""
    global _encoding, _plain
    _encoding = encoding.encoding
    _plain = _getplain()
    _msgcache.clear()
