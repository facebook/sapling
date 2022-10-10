# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# identity.py - program identity
#
# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import functools

from bindings import identity

sniffroot = identity.sniffroot
sniffenv = identity.sniffenv
current = identity.current
default = identity.default


def templatemap():
    currident = default()
    return {
        "@prog@": currident.cliname(),
        "@Product@": currident.productname(),
        "@LongProduct@": currident.longproductname(),
    }


def replace(s):
    """Replace template instances in the given string"""
    if s is not None:
        for template, replacement in templatemap().items():
            s = s.replace(template, replacement)
    return s


@functools.lru_cache
def sniffdir(path):
    return identity.sniffdir(path)
