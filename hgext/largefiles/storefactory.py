# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import re

from mercurial.i18n import _

from mercurial import (
    error,
    hg,
    util,
)

from . import (
    lfutil,
    localstore,
    wirestore,
)

# During clone this function is passed the src's ui object
# but it needs the dest's ui object so it can read out of
# the config file. Use repo.ui instead.
def openstore(repo, remote=None, put=False):
    ui = repo.ui

    if not remote:
        lfpullsource = getattr(repo, 'lfpullsource', None)
        if lfpullsource:
            path = ui.expandpath(lfpullsource)
        elif put:
            path = ui.expandpath('default-push', 'default')
        else:
            path = ui.expandpath('default')

        # ui.expandpath() leaves 'default-push' and 'default' alone if
        # they cannot be expanded: fallback to the empty string,
        # meaning the current directory.
        if path == 'default-push' or path == 'default':
            path = ''
            remote = repo
        else:
            path, _branches = hg.parseurl(path)
            remote = hg.peer(repo, {}, path)

    # The path could be a scheme so use Mercurial's normal functionality
    # to resolve the scheme to a repository and use its path
    path = util.safehasattr(remote, 'url') and remote.url() or remote.path

    match = _scheme_re.match(path)
    if not match:                       # regular filesystem path
        scheme = 'file'
    else:
        scheme = match.group(1)

    try:
        storeproviders = _storeprovider[scheme]
    except KeyError:
        raise error.Abort(_('unsupported URL scheme %r') % scheme)

    for classobj in storeproviders:
        try:
            return classobj(ui, repo, remote)
        except lfutil.storeprotonotcapable:
            pass

    raise error.Abort(_('%s does not appear to be a largefile store') %
                     util.hidepassword(path))

_storeprovider = {
    'file':  [localstore.localstore],
    'http':  [wirestore.wirestore],
    'https': [wirestore.wirestore],
    'ssh': [wirestore.wirestore],
    }

_scheme_re = re.compile(r'^([a-zA-Z0-9+-.]+)://')
