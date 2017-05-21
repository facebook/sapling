# hgdemandimport - global demand-loading of modules for Mercurial
#
# Copyright 2017 Facebook Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''demandimport - automatic demand-loading of modules'''

# This is in a separate package from mercurial because in Python 3,
# demand loading is per-package. Keeping demandimport in the mercurial package
# would disable demand loading for any modules in mercurial.

from __future__ import absolute_import

from . import demandimportpy2 as demandimport

# Re-export.
ignore = demandimport.ignore
isenabled = demandimport.isenabled
enable = demandimport.enable
disable = demandimport.disable
deactivated = demandimport.deactivated
