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

b3 should be listed:
  $ sl log -r $C1 -T '{remotenames}\n'
  origin/b3

  $ sl bookmarks --list-subscriptions
     origin/b3                 3b0ae0a27e72
     origin/main               5d045cb6dd86

Re-sync an existing remote bookmark:

  $ git --git-dir=$TESTTMP/server-repo/.git update-ref refs/heads/b3 $S2
  $ git fetch
  From $TESTTMP/server-repo
   + 3b0ae0a...b1eae93 b3         -> origin/b3  (forced update)

b3 should be updated to S2:
  $ sl log -r origin/b3 -T '{desc}\n'
  S2

Auto pull a remote name that exists in the local Git repo works:

  $ sl log -r origin/b1 -T '{desc}\n'
  pulling 'b1' from '$TESTTMP/server-repo'
  S1

Test remote tags. Prepare it:

  $ git --git-dir=$TESTTMP/server-repo/.git update-ref refs/tags/t1 $S1
  $ git --git-dir=$TESTTMP/server-repo/.git update-ref refs/tags/t2 $S2

Auto pull tags:

  $ sl log -r origin/tags/t1 -T '{desc}\n'
  pulling 'origin/tags/t1', 'tags/t1' from '$TESTTMP/server-repo'
  S1
  $ sl log -r tags/t2 -T '{desc}\n'
  pulling 'tags/t2' from '$TESTTMP/server-repo'
  S2

Explicit pull:

  $ git --git-dir=$TESTTMP/server-repo/.git update-ref refs/tags/t2 $S3
  $ sl pull -B tags/t2
  pulling from $TESTTMP/server-repo
  From $TESTTMP/server-repo
     b1eae93..dd5bc68  dd5bc68f3407e7a490e04ed4c274dbe1bc0e0026 -> refs/remotetags/origin/t2

Push a tag to remote:

  $ sl push -r $S3 --to tags/t3
  To $TESTTMP/server-repo
   * [new tag]         dd5bc68f3407e7a490e04ed4c274dbe1bc0e0026 -> t3
  $ git --git-dir=$TESTTMP/server-repo/.git show-ref --tags
  5d045cb6dd867debc8828c96e248804f892cf171 refs/tags/t1
  dd5bc68f3407e7a490e04ed4c274dbe1bc0e0026 refs/tags/t2
  dd5bc68f3407e7a490e04ed4c274dbe1bc0e0026 refs/tags/t3

Already pulled tags have "<remote>/tags/<tag_name>" remotename and can be resolved via remotename:

  $ sl log -r origin/tags/t2 -r tags/t1 -T '{desc} {remotenames}\n'
  S3 origin/tags/t2 origin/tags/t3
  S1 origin/b1 origin/main origin/tags/t1

The remote tags (refs/remotetags/) and Git tags (refs/tags/) live in two different ref namespaces.
Remote tags do not leak to Git local tags:

  $ git tag --list

Git local tags do not leak to remote tags:

  $ git tag gt1 $S3
  $ sl log -r tags/gt1
  pulling 'tags/gt1' from '$TESTTMP/server-repo'
  abort: unknown revision 'tags/gt1'!
  [255]

