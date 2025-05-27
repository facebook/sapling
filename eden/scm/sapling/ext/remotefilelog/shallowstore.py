# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# shallowstore.py - shallow store for interacting with shallow repos


def wrapstore(store):
    class shallowstore(store.__class__):
        def __contains__(self, path):
            # Assume it exists
            return True

    store.__class__ = shallowstore

    return store
