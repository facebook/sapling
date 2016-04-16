#!/usr/bin/env python
# Copyright 2010 Intevation GmbH
# Author(s):
# Thomas Arendsen Hein <thomas@intevation.de>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Create a Mercurial repository in revlog format 0

changeset:   0:a1ef0b125355
tag:         tip
user:        user
date:        Thu Jan 01 00:00:00 1970 +0000
files:       empty
description:
empty file
"""

from __future__ import absolute_import
import os
import sys

files = [
    ('formatv0/.hg/00changelog.i',
     '000000000000004400000000000000000000000000000000000000'
     '000000000000000000000000000000000000000000000000000000'
     '0000a1ef0b125355d27765928be600cfe85784284ab3'),
    ('formatv0/.hg/00changelog.d',
     '756163613935613961356635353036303562366138343738336237'
     '61623536363738616436356635380a757365720a3020300a656d70'
     '74790a0a656d7074792066696c65'),
    ('formatv0/.hg/00manifest.i',
     '000000000000003000000000000000000000000000000000000000'
     '000000000000000000000000000000000000000000000000000000'
     '0000aca95a9a5f550605b6a84783b7ab56678ad65f58'),
    ('formatv0/.hg/00manifest.d',
     '75656d707479006238306465356431333837353835343163356630'
     '35323635616431343461623966613836643164620a'),
    ('formatv0/.hg/data/empty.i',
     '000000000000000000000000000000000000000000000000000000'
     '000000000000000000000000000000000000000000000000000000'
     '0000b80de5d138758541c5f05265ad144ab9fa86d1db'),
    ('formatv0/.hg/data/empty.d',
     ''),
]

def makedirs(name):
    """recursive directory creation"""
    parent = os.path.dirname(name)
    if parent:
        makedirs(parent)
    os.mkdir(name)

makedirs(os.path.join(*'formatv0/.hg/data'.split('/')))

for name, data in files:
    f = open(name, 'wb')
    f.write(data.decode('hex'))
    f.close()

sys.exit(0)
