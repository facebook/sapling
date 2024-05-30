# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from sapling import error
from sapling.i18n import _

from . import util as ccutil, workspace


def validateimportparams(ui, repo, opts):
    destinationworkspace = opts.get("destination")
    rawdestinationworkspace = opts.get("raw_destination")
    destinationrepo = opts.get("destination_repo")

    if rawdestinationworkspace and destinationworkspace:
        raise error.Abort(
            "conflicting 'destination' and 'raw-destination' options provided"
        )
    elif rawdestinationworkspace:
        destinationworkspace = rawdestinationworkspace
    elif not destinationworkspace or destinationworkspace == ".":
        destinationworkspace = workspace.currentworkspace(repo)
    else:
        destinationworkspace = workspace.userworkspaceprefix(ui) + destinationworkspace

    if not destinationrepo or destinationrepo == ".":
        destinationrepo = ccutil.getreponame(repo)

    sourceworkspace = opts.get("source")
    rawsourceworkspace = opts.get("raw_source")
    sourcerepo = opts.get("source_repo")
    if rawsourceworkspace and sourceworkspace:
        raise error.Abort("conflicting 'source' and 'raw-source' options provided")
    elif rawsourceworkspace:
        sourceworkspace = rawsourceworkspace
    elif not sourceworkspace or sourceworkspace == ".":
        sourceworkspace = workspace.currentworkspace(repo)
    else:
        sourceworkspace = workspace.userworkspaceprefix(ui) + sourceworkspace

    if not sourcerepo or sourcerepo == ".":
        sourcerepo = ccutil.getreponame(repo)
    if sourceworkspace == destinationworkspace and sourcerepo == destinationrepo:
        raise error.Abort(
            _(
                "the source workspace '%s' and the destination workspace '%s' are the same"
            )
            % (sourceworkspace, destinationworkspace)
        )

    return sourceworkspace, destinationworkspace, sourcerepo, destinationrepo
