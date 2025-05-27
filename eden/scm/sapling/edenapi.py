# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


from bindings import edenapi


def getclient(ui, path=None):
    """Obtain the edenapi client"""
    return edenapi.client(ui._rcfg, path=path)
