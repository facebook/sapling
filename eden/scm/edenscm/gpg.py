# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
utilities for GPG support
"""

from typing import Optional


def get_gpg_keyid(ui) -> Optional[str]:
    """If the user has elected to GPG sign commits and has specified a keyid,
    returns the keyid.
    """
    if ui.configbool("gpg", "enabled"):
        key = ui.config("gpg", "key")
        if key:
            # Ensure key is not the empty string.
            return key
    return None
