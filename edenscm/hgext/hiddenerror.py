# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""configurable error messages for accessing hidden changesets

Set the following configuration options to customize the error message
seen when the user attempts to access a hidden changeset::

   [hiddenerror]
   message = my custom message
   hint = my custom hint

The message and hint can contain an optional `{0}` which will be substituted
with the hash of the hidden changeset.
"""
from __future__ import absolute_import

from edenscm.mercurial import context, error, extensions
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import short


testedwith = "ships-with-fb-hgext"


def uisetup(ui):
    """Wrap context.changectx to catch FilteredRepoLookupError."""
    # Get the error messages from the user's configuration and substitute the
    # hash in.
    msgfmt, hintfmt = _getstrings(ui)

    def _filterederror(orig, repo, rev):
        # If the number is beyond the changelog, it's a short hash that
        # just happened to be a number.
        intrev = None
        try:
            intrev = int(rev)
        except ValueError:
            pass
        if intrev is not None and intrev < len(repo):
            node = repo.unfiltered()[rev].node()
            shorthash = short(node)
            msg = msgfmt.format(shorthash)
            hint = hintfmt and hintfmt.format(shorthash)
            return error.FilteredRepoLookupError(msg, hint=hint)
        return orig(repo, rev)

    extensions.wrapfunction(context, "_filterederror", _filterederror)


def _getstrings(ui):
    """Lood the error messages to show when the user tries to access a
       hidden commit from the user's configuration file. Fall back to
       default messages if nothing is configured.
    """
    msg = ui.config("hiddenerror", "message")
    hint = ui.config("hiddenerror", "hint")
    if not msg:
        msg = _("hidden changeset {0}")
    return msg, hint
