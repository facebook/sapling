#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This test was written when we migrated from using C++ Manifests to Rust
# Manifests and wanted to verify the values of the hashes.

  >>> import os, shlex, pprint
  >>> def listcommitandmanifesthashes(rev):
  ...     # returns dictionary from descrition to commit node and manifest node
  ...     # { commit_name: (commit_hash, manifest_hash)}
  ...     template = "{desc} {node|short} {manifest}\n"
  ...     cmd = f"hg log -T '{template}' -r {rev}"
  ...     return list(tuple(line.split()) for line in sheval(cmd).splitlines())

  $ setconfig experimental.allowfilepeer=True
  $ setconfig format.use-segmented-changelog=true
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ . "$TESTDIR/library.sh"

  $ configure dummyssh
  $ enable treemanifest remotenames remotefilelog pushrebase

# Check manifest behavior with empty commit

  $ hginit emptycommit
  $ cd emptycommit
  $ drawdag --no-files << 'EOS'
  > A
  > EOS

  >>> listcommitandmanifesthashes("$A::")
  [('A', '7b3f3d5e5faf', '0000000000000000000000000000000000000000')]

# Check hash and manifest values in a local repository

  $ hginit $TESTTMP/localcommitsandmerge
  $ cd $TESTTMP/localcommitsandmerge

# A - add
# B - modify
# C, D - add + modify
# E - merge with conflict and divergence
# F - just checking that merge doesn't mess repo by performing a modify

  $ drawdag << 'EOS'
  >  # drawdag.defaultfiles=false
  > F   # F/y/c=f  # crash with rustmanifest if y/c=c
  > |
  > E    # E/y/d=(removed)
  > |\   # E/x/a=d
  > C |  # C/y/c=c
  > | |  # C/x/a=c
  > | D  # D/y/d=d
  > |/   # D/x/a=d
  > B  # B/x/b=b
  > |
  > A  # A/x/a=a
  > EOS

  >>> pprint.pprint(listcommitandmanifesthashes("$A::"))
  [('A', '8080f180998f', '47968cf0bfa76dd552b0c468487e0b2e58dd067a'),
   ('B', 'f3631cd323b7', '2e67f334fe3b408e0657bd93b6b0799d8e4bffbf'),
   ('C', 'ab6f17cbfcbc', '9f7dac017ac942faf4c03e81b078194f95a4e042'),
   ('D', 'd55de8a18953', 'e6e729a4a441b3c48a20a19e6696a33428e8824b'),
   ('E', '02d26f311e24', 'c618b8195031a0c6874a557ee7445f6567af4dd7'),
   ('F', 'c431bfe62c4c', 'c8a3f0d6d065d07e6ee7cee3edf15712a7d15d46')]

  $ hg files -r $F
  x/a
  x/b
  y/c
  $ hg cat -r $F x/a x/b y/c
  dbf (no-eol)

# Check that the same graph will be constructed from by pushing commits
# to a server doing pushrebase

  $ hginit $TESTTMP/serverpushrebasemerge
  $ cd $TESTTMP/serverpushrebasemerge
  $ cat >> .hg/hgrc << 'EOF'
  > [extensions]
  > pushrebase=
  > treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > EOF
  $ hg clone 'ssh://user@dummy/serverpushrebasemerge' $TESTTMP/tempclient -q
  $ cd $TESTTMP/tempclient
  $ drawdag << 'EOS'
  >  # drawdag.defaultfiles=false
  > A  # A/x/a=a
  > EOS
  $ hg bookmark master -r $A

  >>> listcommitandmanifesthashes("$A::")
  [('A', '8080f180998f', '47968cf0bfa76dd552b0c468487e0b2e58dd067a')]

  $ hg push -r $A --to master --create
  pushing rev * to destination ssh://user@dummy/serverpushrebasemerge bookmark master (glob)
  searching for changes
  exporting bookmark master
  remote: pushing 1 changeset:
  remote:     *  A (glob)


  $ hg clone 'ssh://user@dummy/serverpushrebasemerge' $TESTTMP/clientpushrebasemerge -q
  fetching tree '' 47968cf0bfa76dd552b0c468487e0b2e58dd067a
  1 trees fetched over 0.00s
  fetching tree 'x' 4f20beec050d22de4f11003f4cdadd266b59be20
  1 trees fetched over 0.00s
  $ cd $TESTTMP/clientpushrebasemerge
  $ cat >> .hg/hgrc << 'EOF'
  > [treemanifest]
  > sendtrees=True
  > treeonly=True
  > EOF
  $ drawdag << 'EOS'
  >  # drawdag.defaultfiles=false
  > F   # F/y/c=f  # crash with rustmanifest if y/c=c
  > |
  > E    # E/y/d=(removed)
  > |\   # E/x/a=d
  > C |  # C/y/c=c
  > | |  # C/x/a=c
  > | D  # D/y/d=d
  > |/   # D/x/a=d
  > B  # B/x/b=b
  > |
  > desc(A)
  > EOS

  >>> pprint.pprint(listcommitandmanifesthashes("$A::"))
  [('A', '8080f180998f', '47968cf0bfa76dd552b0c468487e0b2e58dd067a'),
   ('B', 'f3631cd323b7', '2e67f334fe3b408e0657bd93b6b0799d8e4bffbf'),
   ('C', 'ab6f17cbfcbc', '9f7dac017ac942faf4c03e81b078194f95a4e042'),
   ('D', 'd55de8a18953', 'e6e729a4a441b3c48a20a19e6696a33428e8824b'),
   ('E', '02d26f311e24', 'c618b8195031a0c6874a557ee7445f6567af4dd7'),
   ('F', 'c431bfe62c4c', 'c8a3f0d6d065d07e6ee7cee3edf15712a7d15d46')]

  $ hg push --to=master -r $F
  pushing rev c431bfe62c4c to destination ssh://user@dummy/serverpushrebasemerge bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master
  remote: pushing 5 changesets:
  remote:     *  B (glob)
  remote:     *  D (glob)
  remote:     *  C (glob)
  remote:     *  E (glob)
  remote:     *  F (glob)
  remote: 5 new changesets from the server will be downloaded

  $ hg files -r master
  fetching tree '' 2d97a52179228b1897e02a1f2005e8913fbe284e
  1 trees fetched over 0.00s
  fetching tree 'y' c92bb8214e072555389f3fa53b9bb25df5a7c35a
  1 trees fetched over 0.00s
  x/a
  x/b
  y/c

# Check that a secondary client will pull a consistent view of the repository

  $ hg clone 'ssh://user@dummy/serverpushrebasemerge' $TESTTMP/pullingclient -q
  fetching tree 'x' 34f8d0715188dfecc32838d1d23a93453e0cebd9
  1 trees fetched over 0.00s
  $ cd $TESTTMP/pullingclient

  >>> pprint.pprint(listcommitandmanifesthashes("$A::"))
  [('A', '8080f180998f', '47968cf0bfa76dd552b0c468487e0b2e58dd067a'),
   ('B', 'f3631cd323b7', '2e67f334fe3b408e0657bd93b6b0799d8e4bffbf'),
   ('D', 'd55de8a18953', 'e6e729a4a441b3c48a20a19e6696a33428e8824b'),
   ('C', 'ab6f17cbfcbc', '9f7dac017ac942faf4c03e81b078194f95a4e042'),
   ('E', '0f8253472fd7', '049ce323291d05719ed7194daffe12ef8b7814b2'),
   ('F', '9b380bc27039', '2d97a52179228b1897e02a1f2005e8913fbe284e')]


# Check pushrebase with a branch

  $ cd $TESTTMP/clientpushrebasemerge

# F is master and we will branch from E

  $ drawdag << 'EOS'
  >  # drawdag.defaultfiles=false
  > J   # J/x/a=i
  > |\  # J/y/d=h
  > | I # I/x/a=i
  > | |
  > H | # H/y/d=h
  > |/
  > G   # G/y/d=g
  > |
  > desc(E)
  > EOS
  fetching tree '' 049ce323291d05719ed7194daffe12ef8b7814b2
  1 trees fetched over 0.00s
  fetching tree 'y' bfa15c41434329031f15c569d46b9680cf0f791c
  1 trees fetched over 0.00s
  $ hg push --to=master -r $J
  pushing rev * to destination ssh://user@dummy/serverpushrebasemerge bookmark master (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master
  remote: pushing 4 changesets:
  remote:     *  G (glob)
  remote:     *  H (glob)
  remote:     *  I (glob)
  remote:     *  J (glob)
  remote: 4 new changesets from the server will be downloaded

# Check server after pushrebasing the branch whose parent is E

  $ cd $TESTTMP/serverpushrebasemerge
  $ hg log -G -T '{desc} {bookmarks}'
  o    J master
  ├─╮
  │ o  I
  │ │
  o │  H
  ├─╯
  o  G
  │
  o  F
  │
  o    E
  ├─╮
  │ o  C
  │ │
  o │  D
  ├─╯
  o  B
  │
  o  A

  >>> pprint.pprint(listcommitandmanifesthashes("'desc(F)::'"))
  [('F', '9b380bc27039', '2d97a52179228b1897e02a1f2005e8913fbe284e'),
   ('G', '8895c7a37a32', '466431794675a5fba98f3ccccdd773bcd8ec0f6b'),
   ('H', 'b2b96a821661', 'c1171beb064769767daf0ab34e6fa3424bbc4944'),
   ('I', '597fc1dd8fc9', '14fbfd17f887975a8d7e8250304c52b015e1c5f7'),
   ('J', 'fc81ac1bdb86', '2b28bec49298b3e0e716c99a8c329db5d8ab22e2')]

