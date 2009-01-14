"""
i18n.py - internationalization support for mercurial

Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

import gettext, sys, os

# modelled after templater.templatepath:
if hasattr(sys, 'frozen'):
    module = sys.executable
else:
    module = __file__

base = os.path.dirname(module)
for dir in ('.', '..'):
    localedir = os.path.normpath(os.path.join(base, dir, 'locale'))
    if os.path.isdir(localedir):
        break

t = gettext.translation('hg', localedir, fallback=True)
gettext = t.gettext
_ = gettext
