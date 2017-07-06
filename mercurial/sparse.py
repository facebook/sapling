# sparse.py - functionality for sparse checkouts
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .i18n import _
from . import (
    error,
)

def parseconfig(ui, raw):
    """Parse sparse config file content.

    Returns a tuple of includes, excludes, and profiles.
    """
    includes = set()
    excludes = set()
    current = includes
    profiles = []
    for line in raw.split('\n'):
        line = line.strip()
        if not line or line.startswith('#'):
            # empty or comment line, skip
            continue
        elif line.startswith('%include '):
            line = line[9:].strip()
            if line:
                profiles.append(line)
        elif line == '[include]':
            if current != includes:
                # TODO pass filename into this API so we can report it.
                raise error.Abort(_('sparse config cannot have includes ' +
                                    'after excludes'))
            continue
        elif line == '[exclude]':
            current = excludes
        elif line:
            if line.strip().startswith('/'):
                ui.warn(_('warning: sparse profile cannot use' +
                          ' paths starting with /, ignoring %s\n') % line)
                continue
            current.add(line)

    return includes, excludes, profiles

# Exists as separate function to facilitate monkeypatching.
def readprofile(repo, profile, changeid):
    """Resolve the raw content of a sparse profile file."""
    # TODO add some kind of cache here because this incurs a manifest
    # resolve and can be slow.
    return repo.filectx(profile, changeid=changeid).data()
