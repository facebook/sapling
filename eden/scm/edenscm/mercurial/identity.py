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

prog = "hg"
product = "Mercurial"
longproduct = "Mercurial Distributed SCM"

templatemap = {"@prog@": prog, "@Product@": product, "@LongProduct@": longproduct}


def replace(s):
    """Replace template instances in the given string"""
    if s is not None:
        for template, replacement in templatemap.items():
            s = s.replace(template, replacement)
    return s
