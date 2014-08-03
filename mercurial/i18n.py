# i18n.py - internationalization support for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import encoding
import gettext, sys, os, locale

# modelled after templater.templatepath:
if getattr(sys, 'frozen', None) is not None:
    module = sys.executable
else:
    module = __file__

base = os.path.dirname(module)
for dir in ('.', '..'):
    localedir = os.path.join(base, dir, 'locale')
    if os.path.isdir(localedir):
        break

_languages = None
if (os.name == 'nt'
    and 'LANGUAGE' not in os.environ
    and 'LC_ALL' not in os.environ
    and 'LC_MESSAGES' not in os.environ
    and 'LANG' not in os.environ):
    # Try to detect UI language by "User Interface Language Management" API
    # if no locale variables are set. Note that locale.getdefaultlocale()
    # uses GetLocaleInfo(), which may be different from UI language.
    # (See http://msdn.microsoft.com/en-us/library/dd374098(v=VS.85).aspx )
    try:
        import ctypes
        langid = ctypes.windll.kernel32.GetUserDefaultUILanguage()
        _languages = [locale.windows_locale[langid]]
    except (ImportError, AttributeError, KeyError):
        # ctypes not found or unknown langid
        pass

t = gettext.translation('hg', localedir, _languages, fallback=True)

def gettext(message):
    """Translate message.

    The message is looked up in the catalog to get a Unicode string,
    which is encoded in the local encoding before being returned.

    Important: message is restricted to characters in the encoding
    given by sys.getdefaultencoding() which is most likely 'ascii'.
    """
    # If message is None, t.ugettext will return u'None' as the
    # translation whereas our callers expect us to return None.
    if message is None:
        return message

    if type(message) is unicode:
        # goofy unicode docstrings in test
        paragraphs = message.split(u'\n\n')
    else:
        paragraphs = [p.decode("ascii") for p in message.split('\n\n')]
    # Be careful not to translate the empty string -- it holds the
    # meta data of the .po file.
    u = u'\n\n'.join([p and t.ugettext(p) or '' for p in paragraphs])
    try:
        # encoding.tolocal cannot be used since it will first try to
        # decode the Unicode string. Calling u.decode(enc) really
        # means u.encode(sys.getdefaultencoding()).decode(enc). Since
        # the Python encoding defaults to 'ascii', this fails if the
        # translated string use non-ASCII characters.
        return u.encode(encoding.encoding, "replace")
    except LookupError:
        # An unknown encoding results in a LookupError.
        return message

def _plain():
    if 'HGPLAIN' not in os.environ and 'HGPLAINEXCEPT' not in os.environ:
        return False
    exceptions = os.environ.get('HGPLAINEXCEPT', '').strip().split(',')
    return 'i18n' not in exceptions

if _plain():
    _ = lambda message: message
else:
    _ = gettext
