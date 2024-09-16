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
