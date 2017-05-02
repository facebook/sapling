# policy.py - module policy logic for Mercurial.
#
# Copyright 2015 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import sys

# Rules for how modules can be loaded. Values are:
#
#    c - require C extensions
#    allow - allow pure Python implementation when C loading fails
#    cffi - required cffi versions (implemented within pure module)
#    cffi-allow - allow pure Python implementation if cffi version is missing
#    py - only load pure Python modules
#
# By default, require the C extensions for performance reasons.
policy = b'c'
policynoc = (b'cffi', b'cffi-allow', b'py')
policynocffi = (b'c', b'py')

try:
    from . import __modulepolicy__
    policy = __modulepolicy__.modulepolicy
except ImportError:
    pass

# PyPy doesn't load C extensions.
#
# The canonical way to do this is to test platform.python_implementation().
# But we don't import platform and don't bloat for it here.
if r'__pypy__' in sys.builtin_module_names:
    policy = b'cffi'

# Our C extensions aren't yet compatible with Python 3. So use pure Python
# on Python 3 for now.
if sys.version_info[0] >= 3:
    policy = b'py'

# Environment variable can always force settings.
if sys.version_info[0] >= 3:
    if r'HGMODULEPOLICY' in os.environ:
        policy = os.environ[r'HGMODULEPOLICY'].encode(r'utf-8')
else:
    policy = os.environ.get(r'HGMODULEPOLICY', policy)
