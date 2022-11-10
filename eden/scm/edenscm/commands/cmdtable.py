# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from typing import List, Optional

import bindings

from .. import identity, registrar


table = bindings.commands.table()
command = registrar.command(table)

show_legacy_aliases = "hg" in identity.default().cliname()


def command_name(
    name: str,
    alias: Optional[List[str]] = None,
    legacy_alias: Optional[List[str]] = None,
) -> str:
    """Creates a string suitable for use as the first argument to @command().

    name: primary name of the command. This will be displayed in `@prog help commands`,
          in the URL for the docs on the website, etc.
    alias: list of supported aliases for the command.
    legacy_alias: list of legacy aliases for the command for historical Mercurial
                  users.
    """
    all_names = [name] + (alias or [])
    if show_legacy_aliases and legacy_alias:
        all_names += legacy_alias
    return "|".join(all_names)
