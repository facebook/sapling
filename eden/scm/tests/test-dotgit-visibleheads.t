#require git no-windows

Test visibleheads sync between Git and Sl (dotgit).

  $ . $TESTDIR/git.sh

  $ git init -qb main client-repo
  $ cd client-repo

Add some commits:

  $ HGIDENTITY=sl drawdag << 'EOS'
  > B C
  > |/
  > A
  > EOS

They become visible heads:

  $ git show-ref
  0de30934572f96ff6d3cbfc70aa8b46ef95dbb42 refs/visibleheads/0de30934572f96ff6d3cbfc70aa8b46ef95dbb42
  5d38a953d58b0c80a4416ba62e62d3f2985a3726 refs/visibleheads/5d38a953d58b0c80a4416ba62e62d3f2985a3726

  $ sl log -r 'heads(draft())' -T '{desc} {node}\n'
  B 0de30934572f96ff6d3cbfc70aa8b46ef95dbb42
  C 5d38a953d58b0c80a4416ba62e62d3f2985a3726

Hiding a commit removes it from visibleheads:

  $ sl hide -q $B

  $ git show-ref
  5d38a953d58b0c80a4416ba62e62d3f2985a3726 refs/visibleheads/5d38a953d58b0c80a4416ba62e62d3f2985a3726

  $ sl log -r 'heads(draft())' -T '{desc} {node}\n'
  C 5d38a953d58b0c80a4416ba62e62d3f2985a3726
