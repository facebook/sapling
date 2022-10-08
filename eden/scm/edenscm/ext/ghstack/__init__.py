# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""ghstack for Sapling (EXPERIMENTAL)
"""

from edenscm import registrar
from edenscm.i18n import _

cmdtable = {}
command = registrar.command(cmdtable)

import ghstack


@command(
    "ghstack",
    [],
    _("SUBCOMMAND ..."),
)
def ghstack_command(ui, repo, *pats, **opts) -> None:
    print(ghstack.__version__)
