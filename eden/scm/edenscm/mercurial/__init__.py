# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# __init__.py - Startup and module loading logic for Mercurial.
#
# Copyright 2015 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import sys

# Allow 'from mercurial import demandimport' to keep working.
from edenscm import hgdemandimport


# pyre-fixme[11]: Annotation `hgdemandimport` is not defined as a type.
demandimport = hgdemandimport

__all__ = []

if getattr(sys, "platform") == "win32":
    configdir = os.path.join(
        getattr(os, "environ").get("PROGRAMDATA") or r"\ProgramData",
        "Facebook",
        "Mercurial",
    )
else:
    configdir = "/etc/mercurial"


def shoulduselegacy(name):
    legacy = getattr(os, "environ").get("HGLEGACY")
    if legacy is not None:
        return name in legacy.split()
    else:
        return os.path.lexists(os.path.join(configdir, "legacy.%s" % name))
