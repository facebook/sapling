#require git no-windows

Test visibleheads sync between Git and Sl (dotgit).

  $ . $TESTDIR/git.sh

  $ git init -qb main client-repo
  $ cd client-repo

Add some commits:

  $ HGIDENTITY=sl drawdag << 'EOS'
  >   D
  >   |
  > B C
  > |/
  > A
  > EOS

They become visible heads:

  $ git show-ref
  0de30934572f96ff6d3cbfc70aa8b46ef95dbb42 refs/visibleheads/0de30934572f96ff6d3cbfc70aa8b46ef95dbb42
  5e987cb91d3a6d4e42726b701c4ac053755eb2c9 refs/visibleheads/5e987cb91d3a6d4e42726b701c4ac053755eb2c9

  $ sl log -r 'heads(draft())' -T '{desc} {node}\n'
  B 0de30934572f96ff6d3cbfc70aa8b46ef95dbb42
  D 5e987cb91d3a6d4e42726b701c4ac053755eb2c9

Hiding a commit removes it from visibleheads:

  $ sl hide -q $B

  $ git show-ref
  5e987cb91d3a6d4e42726b701c4ac053755eb2c9 refs/visibleheads/5e987cb91d3a6d4e42726b701c4ac053755eb2c9

  $ sl log -r 'heads(draft())' -T '{desc} {node}\n'
  D 5e987cb91d3a6d4e42726b701c4ac053755eb2c9

Folding:

  $ sl up -q $D
  $ sl fold -q --exact -r $C+$D
  $ git show-ref
  f99f35f848e008a864277632059e3c45dc7a92e6 refs/visibleheads/f99f35f848e008a864277632059e3c45dc7a92e6
  $ sl log -r 'heads(draft())' -T '{desc|firstline} {node}\n'
  C f99f35f848e008a864277632059e3c45dc7a92e6

Metaediting, should not keep obsoleted commits visible:

  $ sl metaedit -m C1
  $ sl metaedit -m C2
  $ sl log -r 'heads(draft())' -T '{desc|firstline} {node}\n'
  C2 c3755c9e79b57f610a0bc0aa98426723a0145ab8
  $ git show-ref
  c3755c9e79b57f610a0bc0aa98426723a0145ab8 refs/visibleheads/c3755c9e79b57f610a0bc0aa98426723a0145ab8
  $ sl log -Gr 'all()' -T '{desc}'
  @  C2
  │
  o  A

Reviving the obsoleted commit:

  $ sl bookmark -r 'desc(C1)' b1
  $ sl log -Gr 'all()' -T '{desc|firstline}'
  @  C2
  │
  │ x  C1
  ├─╯
  o  A

Hiding the obsoleted commit:

  $ sl hide 'obsolete()'
  hiding commit f8f3ef7675c7 "C1"
  1 changeset hidden
  removing bookmark 'b1' (was at: f8f3ef7675c7)
  1 bookmark removed
  $ sl log -r 'heads(draft())' -T '{desc|firstline} {node}\n'
  C2 c3755c9e79b57f610a0bc0aa98426723a0145ab8

Another repo where the "current commit" is also referred by Git local branches:

  $ cd
  $ git init -qb main client-repo2
  $ cd client-repo2
  $ git commit -qm A --allow-empty
  $ git commit -qm B --allow-empty
  $ git checkout -q -b branch2
  $ git checkout -q -b branch3

  $ sl log -Gr 'all()' -T '{desc|firstline} {bookmarks}'
  @  B branch2 branch3
  │
  o  A

Metaedit:

  $ sl metaedit -m B2

Local bookmarks "branch2", "branch3" are updated.
FIXME: "B" should be hidden. But it is pinned by the "main" branch

  $ sl log -Gr 'all()' -T '{desc|firstline} {bookmarks}'
  @  B2 branch2 branch3
  │
  │ x  B
  ├─╯
  o  A

Adding a bookmark and git "main" ref to the old commit to make it visible:

  $ B=$(sl log -r 'predecessors(.)-.' -T '{node}')
  $ sl bookmark -r $B revive1
  $ git update-ref refs/heads/main $B
  $ sl log -Gr 'all()' -T '{desc|firstline} {bookmarks}'
  @  B2 branch2 branch3
  │
  │ x  B revive1
  ├─╯
  o  A

Hiding the obsolete commit:

  $ sl hide 'obsolete()'
  hiding commit ac23eb7fe089 "B"
  1 changeset hidden
  removing bookmark 'revive1' (was at: ac23eb7fe089)
  1 bookmark removed

FIXME: "B" is still visible
  $ sl log -Gr 'all()' -T '{desc|firstline} {bookmarks}'
  @  B2 branch2 branch3
  │
  │ x  B
  ├─╯
  o  A
