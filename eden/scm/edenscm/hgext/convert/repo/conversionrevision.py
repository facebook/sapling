# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# conversionrevision.py - (EXPERIMENTAL)

from __future__ import absolute_import

import collections

from edenscm.mercurial import node as nodemod


class conversionrevision(
    collections.namedtuple(
        "conversionrevision", ["variant", "sourcehash", "sourceproject", "destpath"]
    )
):
    """Represents a unique mapping of a single commit from source to
    destination. Immutable"""

    VARIANT_NONE = "N"  # Don't use: Used only for representing the "none" rev
    VARIANT_ROOTED = "R"  # Used for commits migrated to root directory
    VARIANT_DIRRED = "D"  # Used for commits migrated to manifest directory
    VARIANT_UNIFIED = "U"  # Used for commits merged into a single destination history

    NONE = None

    @classmethod
    def _classinit(cls):
        # type: () -> None
        """Initialize class members"""
        cls.NONE = conversionrevision(
            conversionrevision.VARIANT_NONE, nodemod.nullhex, "", ""
        )

    @classmethod
    def parse(cls, revstring):
        """Parses the string representation of a conversionrevision into an object"""
        variant = revstring[0:1]
        sourcehash = revstring[1:41]
        separatorindex = revstring.index(":")
        sourceproject = revstring[41:separatorindex]
        destpath = revstring[separatorindex + 1 :]
        return conversionrevision(variant, sourcehash, sourceproject, destpath)

    def __str__(self):
        # type: () -> str
        return "%s%s%s:%s" % (
            self.variant,
            self.sourcehash,
            self.sourceproject,
            self.destpath,
        )


conversionrevision._classinit()
