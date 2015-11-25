# __init__.py - Startup and module loading logic for Mercurial.
#
# Copyright 2015 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import imp
import os
import sys

__all__ = []

# Rules for how modules can be loaded. Values are:
#
#    c - require C extensions
#    allow - allow pure Python implementation when C loading fails
#    py - only load pure Python modules
modulepolicy = '@MODULELOADPOLICY@'

# By default, require the C extensions for performance reasons.
if modulepolicy == '@' 'MODULELOADPOLICY' '@':
    modulepolicy = 'c'

# Environment variable can always force settings.
modulepolicy = os.environ.get('HGMODULEPOLICY', modulepolicy)

# Modules that have both Python and C implementations. See also the
# set of .py files under mercurial/pure/.
_dualmodules = set([
    'mercurial.base85',
    'mercurial.bdiff',
    'mercurial.diffhelpers',
    'mercurial.mpatch',
    'mercurial.osutil',
    'mercurial.parsers',
])

class hgimporter(object):
    """Object that conforms to import hook interface defined in PEP-302."""
    def find_module(self, name, path=None):
        # We only care about modules that have both C and pure implementations.
        if name in _dualmodules:
            return self
        return None

    def load_module(self, name):
        mod = sys.modules.get(name, None)
        if mod:
            return mod

        mercurial = sys.modules['mercurial']

        # Unlike the default importer which searches special locations and
        # sys.path, we only look in the directory where "mercurial" was
        # imported from.

        # imp.find_module doesn't support submodules (modules with ".").
        # Instead you have to pass the parent package's __path__ attribute
        # as the path argument.
        stem = name.split('.')[-1]

        try:
            if modulepolicy == 'py':
                raise ImportError()

            modinfo = imp.find_module(stem, mercurial.__path__)

            # The Mercurial installer used to copy files from
            # mercurial/pure/*.py to mercurial/*.py. Therefore, it's possible
            # for some installations to have .py files under mercurial/*.
            # Loading Python modules when we expected C versions could result
            # in a) poor performance b) loading a version from a previous
            # Mercurial version, potentially leading to incompatibility. Either
            # scenario is bad. So we verify that modules loaded from
            # mercurial/* are C extensions. If the current policy allows the
            # loading of .py modules, the module will be re-imported from
            # mercurial/pure/* below.
            if modinfo[2][2] != imp.C_EXTENSION:
                raise ImportError('.py version of %s found where C '
                                  'version should exist' % name)

        except ImportError:
            if modulepolicy == 'c':
                raise

            # Could not load the C extension and pure Python is allowed. So
            # try to load them.
            from . import pure
            modinfo = imp.find_module(stem, pure.__path__)
            if not modinfo:
                raise ImportError('could not find mercurial module %s' %
                                  name)

        mod = imp.load_module(name, *modinfo)
        sys.modules[name] = mod
        return mod

# We automagically register our custom importer as a side-effect of loading.
# This is necessary to ensure that any entry points are able to import
# mercurial.* modules without having to perform this registration themselves.
if not any(isinstance(x, hgimporter) for x in sys.meta_path):
    # meta_path is used before any implicit finders and before sys.path.
    sys.meta_path.insert(0, hgimporter())
