#require git no-windows

Test remote names can be added and updated:

  $ . $TESTDIR/git.sh

These configs are set by builtin:dotgit. However, the test background config
overrides it. Set them back for this test.

  $ setconfig remotenames.rename.default=origin remotenames.hoist=origin

Prepare a server repo with some branch names:

  $ git init -q -b main server-repo
  $ cd server-repo
  $ HGIDENTITY=sl drawdag << 'EOS'
  > S1..S4
  > EOF
  $ git update-ref refs/heads/main $S1
  $ for i in b1 b2; do
  >   git update-ref refs/heads/$i $S1
  > done

Clone the repo:

  $ cd
  $ git clone -q server-repo client-repo
  $ cd client-repo

By default, `sl` only syncs the "main" remote branch:

  $ sl log -r . -T '{remotenames}\n'
  origin/main

  $ sl bookmarks --list-subscriptions
     origin/main               5d045cb6dd86

Git tracks more references:

  $ git for-each-ref | grep origin/b
  5d045cb6dd867debc8828c96e248804f892cf171 commit	refs/remotes/origin/b1
  5d045cb6dd867debc8828c96e248804f892cf171 commit	refs/remotes/origin/b2

Push creates remote bookmarks:

  $ HGIDENTITY=sl drawdag << 'EOS'
  > .-C1..C3
  > EOS
  $ sl push -r $C1 --to b3
  To $TESTTMP/server-repo
   * [new branch]      3b0ae0a27e72b7be322cc30dd57eaf88f9ddfa2d -> b3

FIXME: b3 should be listed:
  $ sl log -r $C1 -T '{remotenames}\n'

  $ sl bookmarks --list-subscriptions
     origin/main               5d045cb6dd86

Re-sync an existing remote bookmark:

  $ git --git-dir=$TESTTMP/server-repo/.git update-ref refs/heads/b3 $S2
  $ git fetch
  From $TESTTMP/server-repo
   + 3b0ae0a...b1eae93 b3         -> origin/b3  (forced update)

FIXME: b3 should be updated to S2:
  $ sl log -r origin/b3 -T '{desc}\n'
  pulling 'b3' from '$TESTTMP/server-repo'
  abort: unknown revision 'origin/b3'!
  [255]

Auto pull a remote name that exists in the local Git repo works:
FIXME: This does not work right now:
  $ sl log -r origin/b1 -T '{desc}\n'
  pulling 'b1' from '$TESTTMP/server-repo'
  abort: unknown revision 'origin/b1'!
  [255]
