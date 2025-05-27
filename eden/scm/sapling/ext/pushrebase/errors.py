# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# errors.py - errors used by pushrebase


from sapling import error
from sapling.i18n import _


class ConflictsError(error.Abort):
    def __init__(self, conflicts):
        self.conflicts = conflicts
        msg = (
            _("conflicting changes in:\n%s\n")
            % "".join("    %s\n" % f for f in sorted(conflicts))
        ).strip()
        hint = _("pull and rebase your changes locally, then try again")
        super(ConflictsError, self).__init__(msg, hint=hint)


class StackPushUnsupportedError(error.Abort):
    """The push cannot be done via stackpush"""
