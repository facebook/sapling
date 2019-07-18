# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
sh % "cat" << r'''
"""A small extension to test bundle2 pushback parts.
Current bundle2 implementation doesn't provide a way to generate those
parts, so they must be created by extensions.
"""
from __future__ import absolute_import
from edenscm.mercurial import bundle2, exchange, pushkey, util
def _newhandlechangegroup(op, inpart):
    """This function wraps the changegroup part handler for getbundle.
    It issues an additional pushkey part to send a new
    bookmark back to the client"""
    result = bundle2.handlechangegroup(op, inpart)
    if 'pushback' in op.reply.capabilities:
        params = {'namespace': 'bookmarks',
                  'key': 'new-server-mark',
                  'old': '',
                  'new': 'tip'}
        encodedparams = [(k, pushkey.encode(v)) for (k,v) in params.items()]
        op.reply.newpart('pushkey', mandatoryparams=encodedparams)
    else:
        op.reply.newpart('output', data='pushback not enabled')
    return result
_newhandlechangegroup.params = bundle2.handlechangegroup.params
bundle2.parthandlermapping['changegroup'] = _newhandlechangegroup
''' > "bundle2.py"

sh % "cat" << r"""
[ui]
ssh = $PYTHON "$TESTDIR/dummyssh"
username = nobody <no.reply@example.com>
""" >> "$HGRCPATH"

# Set up server repository

sh % "hg init server"
sh % "cd server"
sh % "echo c0" > "f0"
sh % "hg commit -Am 0" == "adding f0"

# Set up client repository

sh % "cd .."
sh % "hg clone 'ssh://user@dummy/server' client -q"
sh % "cd client"

# Enable extension
sh % "cat" << r"""
[extensions]
bundle2=$TESTTMP/bundle2.py
""" >> "$HGRCPATH"

# Without config

sh % "cd ../client"
sh % "echo c1" > "f1"
sh % "hg commit -Am 1" == "adding f1"
sh % "hg push" == r"""
    pushing to ssh://user@dummy/server
    searching for changes
    remote: adding changesets
    remote: adding manifests
    remote: adding file changes
    remote: added 1 changesets with 1 changes to 1 files
    remote: pushback not enabled"""
sh % "hg bookmark" == "no bookmarks set"

sh % "cd ../server"
sh % "tglogp" == r"""
    o  1: 2b9c7234e035 public '1'
    |
    @  0: 6cee5c8f3e5b public '0'"""


# With config

sh % "cd ../client"
sh % "echo '[experimental]'" >> ".hg/hgrc"
sh % "echo 'bundle2.pushback = True'" >> ".hg/hgrc"
sh % "echo c2" > "f2"
sh % "hg commit -Am 2" == "adding f2"
sh % "hg push" == r"""
    pushing to ssh://user@dummy/server
    searching for changes
    remote: adding changesets
    remote: adding manifests
    remote: adding file changes
    remote: added 1 changesets with 1 changes to 1 files"""
sh % "hg bookmark" == "   new-server-mark           2:0a76dfb2e179"

sh % "cd ../server"
sh % "tglogp" == r"""
    o  2: 0a76dfb2e179 public '2'
    |
    o  1: 2b9c7234e035 public '1'
    |
    @  0: 6cee5c8f3e5b public '0'"""
