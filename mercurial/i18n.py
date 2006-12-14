"""
i18n.py - internationalization support for mercurial

Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

import gettext
t = gettext.translation('hg', fallback=1)
gettext = t.gettext
_ = gettext
