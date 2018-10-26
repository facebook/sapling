# Copyright 2005 Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# name space package to host third party extensions
from __future__ import absolute_import

import pkgutil


__path__ = pkgutil.extend_path(__path__, __name__)
