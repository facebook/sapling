# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# If we are running in the "sapling" context, use Sapling's BufIO
# instead of BytesIO. BufIO interplays better with Sapling's internal IO.
try:
    import bindings

    BufIO = bindings.io.BufIO  # camelcase-required
except ImportError:
    import io

    BufIO = io.BytesIO  # camelcase-required
