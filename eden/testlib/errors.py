# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


class MissingCommitError(LookupError):
    """Raised when failing to find some commit"""

    pass


class AmbiguousCommitError(LookupError):
    """Raised when failing to identify a commit due to ambiguity"""

    pass
