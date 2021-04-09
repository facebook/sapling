# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# error: errors used in fastannotate


class CorruptedFileError(Exception):
    pass


class CannotReuseError(Exception):
    """cannot reuse or update the cache incrementally"""
