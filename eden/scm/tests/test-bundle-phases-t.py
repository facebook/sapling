# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'experimental.bundle-phases=yes'"

# Set up repo with linear history
sh % "hg init linear"
sh % "cd linear"
sh % "drawdag" << r"""
E
|
D
|
C
|
B
|
A
"""
sh % "hg phase --public $A"
sh % "hg phase --force --secret $D"
sh % "hg log -G -T '{desc} {phase}\\n'" == r"""
    o  E secret
    |
    o  D secret
    |
    o  C draft
    |
    o  B draft
    |
    o  A public"""
# Phases are restored when unbundling
sh % "hg bundle --base $B -r $E bundle" == "3 changesets found"
sh % "hg debugbundle bundle" == r"""
    Stream params: {Compression: BZ}
    changegroup -- {nbchanges: 3, targetphase: 2, version: 02}
        26805aba1e600a82e93661149f2313866a221a7b
        f585351a92f85104bff7c284233c338b10eb1df7
        9bc730a19041f9ec7cb33c626e811aa233efb18c
    phase-heads -- {}
        26805aba1e600a82e93661149f2313866a221a7b draft
    b2x:treegroup2 -- {cache: False, category: manifests, version: 1}
        3 data items, 3 history items
        593f80c06c15ea10c6d767d660338eb0ba2a50b5 
        630382bb478b2100590cff985f13084f7e77545e 
        7c9b4fd8b49377e2fead2e9610bb8db910a98c53"""
sh % "hg debugstrip --no-backup $C"
sh % "hg unbundle -q bundle"
sh % "rm bundle"
sh % "hg log -G -T '{desc} {phase}\\n'" == r"""
    o  E secret
    |
    o  D secret
    |
    o  C draft
    |
    o  B draft
    |
    o  A public"""
# Root revision's phase is preserved
sh % "hg bundle -a bundle" == "5 changesets found"
sh % "hg debugstrip --no-backup $A"
sh % "hg unbundle -q bundle"
sh % "rm bundle"
sh % "hg log -G -T '{desc} {phase}\\n'" == r"""
    o  E secret
    |
    o  D secret
    |
    o  C draft
    |
    o  B draft
    |
    o  A public"""
# Completely public history can be restored
sh % "hg phase --public $E"
sh % "hg bundle -a bundle" == "5 changesets found"
sh % "hg debugstrip --no-backup $A"
sh % "hg unbundle -q bundle"
sh % "rm bundle"
sh % "hg log -G -T '{desc} {phase}\\n'" == r"""
    o  E public
    |
    o  D public
    |
    o  C public
    |
    o  B public
    |
    o  A public"""
# Direct transition from public to secret can be restored
sh % "hg phase --secret --force $D"
sh % "hg bundle -a bundle" == "5 changesets found"
sh % "hg debugstrip --no-backup $A"
sh % "hg unbundle -q bundle"
sh % "rm bundle"
sh % "hg log -G -T '{desc} {phase}\\n'" == r"""
    o  E secret
    |
    o  D secret
    |
    o  C public
    |
    o  B public
    |
    o  A public"""
# Revisions within bundle preserve their phase even if parent changes its phase
sh % "hg phase --draft --force $B"
sh % "hg bundle --base $B -r $E bundle" == "3 changesets found"
sh % "hg debugstrip --no-backup $C"
sh % "hg phase --public $B"
sh % "hg unbundle -q bundle"
sh % "rm bundle"
sh % "hg log -G -T '{desc} {phase}\\n'" == r"""
    o  E secret
    |
    o  D secret
    |
    o  C draft
    |
    o  B public
    |
    o  A public"""
# Phase of ancestors of stripped node get advanced to accommodate child
sh % "hg bundle --base $B -r $E bundle" == "3 changesets found"
sh % "hg debugstrip --no-backup $C"
sh % "hg phase --force --secret $B"
sh % "hg unbundle -q bundle"
sh % "rm bundle"
sh % "hg log -G -T '{desc} {phase}\\n'" == r"""
    o  E secret
    |
    o  D secret
    |
    o  C draft
    |
    o  B draft
    |
    o  A public"""
# Unbundling advances phases of changesets even if they were already in the repo.
# To test that, create a bundle of everything in draft phase and then unbundle
# to see that secret becomes draft, but public remains public.
sh % "hg phase --draft --force $A"
sh % "hg phase --draft $E"
sh % "hg bundle -a bundle" == "5 changesets found"
sh % "hg phase --public $A"
sh % "hg phase --secret --force $E"
sh % "hg unbundle -q bundle"
sh % "rm bundle"
sh % "hg log -G -T '{desc} {phase}\\n'" == r"""
    o  E draft
    |
    o  D draft
    |
    o  C draft
    |
    o  B draft
    |
    o  A public"""
# Unbundling change in the middle of a stack does not affect later changes
sh % "hg debugstrip --no-backup $E"
sh % "hg phase --secret --force $D"
sh % "hg log -G -T '{desc} {phase}\\n'" == r"""
    o  D secret
    |
    o  C draft
    |
    o  B draft
    |
    o  A public"""
sh % "hg bundle --base $A -r $B bundle" == "1 changesets found"
sh % "hg unbundle -q bundle"
sh % "rm bundle"
sh % "hg log -G -T '{desc} {phase}\\n'" == r"""
    o  D secret
    |
    o  C draft
    |
    o  B draft
    |
    o  A public"""

sh % "cd .."

# Set up repo with non-linear history
sh % "hg init non-linear"
sh % "cd non-linear"
sh % "drawdag" << r"""
D E
|\|
B C
|/
A
"""
sh % "hg phase --public $C"
sh % "hg phase --force --secret $B"
sh % "hg log -G -T '{node|short} {desc} {phase}\\n'" == r"""
    o  03ca77807e91 E draft
    |
    | o  4e4f9194f9f1 D secret
    |/|
    o |  dc0947a82db8 C public
    | |
    | o  112478962961 B secret
    |/
    o  426bada5c675 A public"""

# Restore bundle of entire repo
sh % "hg bundle -a bundle" == "5 changesets found"
sh % "hg debugbundle bundle" == r"""
    Stream params: {Compression: BZ}
    changegroup -- {nbchanges: 5, targetphase: 2, version: 02}
        426bada5c67598ca65036d57d9e4b64b0c1ce7a0
        112478962961147124edd43549aedd1a335e44bf
        dc0947a82db884575bb76ea10ac97b08536bfa03
        4e4f9194f9f181c57f62e823e8bdfa46ab9e4ff4
        03ca77807e919db8807c3749086dc36fb478cac0
    phase-heads -- {}
        dc0947a82db884575bb76ea10ac97b08536bfa03 public
        03ca77807e919db8807c3749086dc36fb478cac0 draft
    b2x:treegroup2 -- {cache: False, category: manifests, version: 1}
        5 data items, 5 history items
        0dcb7c35662e492669ca486f9ccd64d15e356c39 
        41b34f08c1356f6ad068e9ab9b43d984245111aa 
        5a538d6dd01b4058a549747c7947ce2dbf29f2ae 
        eb79886383871977bccdb3000c275a279f0d4c99 
        ebe5c7fbd6d257f80727ddde43cfee5f1ea4677e"""
sh % "hg debugstrip --no-backup $A"
sh % "hg unbundle -q bundle"
sh % "rm bundle"
sh % "hg log -G -T '{node|short} {desc} {phase}\\n'" == r"""
    o  03ca77807e91 E draft
    |
    | o  4e4f9194f9f1 D secret
    |/|
    o |  dc0947a82db8 C public
    | |
    | o  112478962961 B secret
    |/
    o  426bada5c675 A public"""

sh % "hg bundle --base $A+$C -r $D bundle" == "2 changesets found"
sh % "hg debugbundle bundle" == r"""
    Stream params: {Compression: BZ}
    changegroup -- {nbchanges: 2, targetphase: 2, version: 02}
        112478962961147124edd43549aedd1a335e44bf
        4e4f9194f9f181c57f62e823e8bdfa46ab9e4ff4
    phase-heads -- {}
    b2x:treegroup2 -- {cache: False, category: manifests, version: 1}
        2 data items, 2 history items
        0dcb7c35662e492669ca486f9ccd64d15e356c39 
        eb79886383871977bccdb3000c275a279f0d4c99"""
sh % "rm bundle"

sh % "hg bundle --base $A -r $D bundle" == "3 changesets found"
sh % "hg debugbundle bundle" == r"""
    Stream params: {Compression: BZ}
    changegroup -- {nbchanges: 3, targetphase: 2, version: 02}
        112478962961147124edd43549aedd1a335e44bf
        dc0947a82db884575bb76ea10ac97b08536bfa03
        4e4f9194f9f181c57f62e823e8bdfa46ab9e4ff4
    phase-heads -- {}
        dc0947a82db884575bb76ea10ac97b08536bfa03 public
    b2x:treegroup2 -- {cache: False, category: manifests, version: 1}
        3 data items, 3 history items
        0dcb7c35662e492669ca486f9ccd64d15e356c39 
        5a538d6dd01b4058a549747c7947ce2dbf29f2ae 
        eb79886383871977bccdb3000c275a279f0d4c99"""
sh % "rm bundle"

sh % "hg bundle --base $B+$C -r $D+$E bundle" == "2 changesets found"
sh % "hg debugbundle bundle" == r"""
    Stream params: {Compression: BZ}
    changegroup -- {nbchanges: 2, targetphase: 2, version: 02}
        4e4f9194f9f181c57f62e823e8bdfa46ab9e4ff4
        03ca77807e919db8807c3749086dc36fb478cac0
    phase-heads -- {}
        03ca77807e919db8807c3749086dc36fb478cac0 draft
    b2x:treegroup2 -- {cache: False, category: manifests, version: 1}
        2 data items, 2 history items
        0dcb7c35662e492669ca486f9ccd64d15e356c39 
        ebe5c7fbd6d257f80727ddde43cfee5f1ea4677e"""
sh % "rm bundle"
