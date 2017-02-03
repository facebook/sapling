# hiddenhash.py
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""friendlier error messages for filtered lookup errors"""

testedwith = 'ships-with-fb-hgext'

import re

from mercurial import (
    context,
    error,
    extensions,
)

from mercurial.i18n import _
from mercurial.node import short

def uisetup(ui):
    extensions.wrapfunction(context, 'changectx', _wrapchangectx)

def _wrapchangectx(orig, repo, *args, **kwargs):
    """Edit the error message for FilteredRepoLookupError to show a
       hash instead of a rev number, and don't suggest using --hidden.
    """
    try:
        return orig(repo, *args, **kwargs)
    except error.FilteredRepoLookupError as e:
        match = re.match(r"hidden revision '(\d+)'", e.message)
        if not match:
            raise
        rev = int(match.group(1))
        node = repo.unfiltered().changelog.node(rev)
        msg = _("hidden changeset %s") % short(node)
        raise error.FilteredRepoLookupError(msg)
