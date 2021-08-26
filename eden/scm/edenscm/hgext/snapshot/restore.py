# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


def restore(ui, repo, csid, **opts):
    ui.status(f"Will restore snapshot {csid}\n", component="snapshot")
