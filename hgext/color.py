# color.py color output for Mercurial commands
#
# Copyright (C) 2007 Kevin Christen <kevin.christen@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''enable Mercurial color mode (DEPRECATED)

This extensions enable Mercurial color mode. The feature is now directly
available in Mercurial core. You can access it using::

  [ui]
  color = auto

See :hg:`help color` for details.
'''

from __future__ import absolute_import

from mercurial import color

# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

def extsetup(ui):
    # change default color config
    color._enabledbydefault = True
