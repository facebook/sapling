# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from bindings import lz4


lz4compress = lz4.compress
lz4compresshc = lz4.compresshc
lz4decompress = lz4.decompress
