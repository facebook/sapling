# hiddenerror.py
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""configurable error messages for accessing hidden changesets

   Set the following configuration options to customize the error message
   seen when the user attempts to access a hidden changeset::

   [hiddenerror]
   message = my custom message
   hint = my custom hint

   The message and hint can contain an optional `{0}` which will be substituted
   with the hash of the hidden changeset.
"""

import re

from mercurial import context, error
from mercurial.i18n import _
from mercurial.node import short

testedwith = 'ships-with-fb-hgext'

def uisetup(ui):
    """Wrap context.changectx to catch FilteredRepoLookupError."""
    class changectxwrapper(context.changectx):
        def __init__(self, repo, *args, **kwargs):
            try:
                # Attempt to call constructor normally.
                super(changectxwrapper, self).__init__(repo, *args, **kwargs)
            except error.FilteredRepoLookupError as e:
                # If we get a FilteredRepoLookupError, attempt to rewrite
                # the error message and re-raise the exception.
                match = re.match(r"hidden revision '(\d+)'", str(e))
                if not match:
                    raise

                rev = int(match.group(1))
                cl = repo.unfiltered().changelog

                # If the number is beyond the changelog, it's a short hash that
                # just happened to be a number.
                if rev >= len(cl):
                    raise

                node = cl.node(rev)
                shorthash = short(node)

                # Get the error messages from the user's configuration and
                # substitute the hash in. Use a dict for the hint argument
                # to make it optional via keyword argument unpacking.
                msg, hint = _getstrings(ui)
                msg = msg.format(shorthash)
                hintarg = {}
                if hint:
                    hintarg['hint'] = hint.format(shorthash)

                raise error.FilteredRepoLookupError(msg, **hintarg)
    setattr(context, 'changectx', changectxwrapper)

def _getstrings(ui):
    """Lood the error messages to show when the user tries to access a
       hidden commit from the user's configuration file. Fall back to
       default messages if nothing is configured.
    """
    msg = ui.config('hiddenerror', 'message')
    hint = ui.config('hiddenerror', 'hint')
    if not msg:
        msg = _("hidden changeset {0}")
    return msg, hint
