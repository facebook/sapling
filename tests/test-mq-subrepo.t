  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > commitsubrepos = Yes
  > [extensions]
  > mq =
  > record =
  > [diff]
  > nodates = 1
  > EOF

  $ stdin=`pwd`/stdin.tmp

fn to create new repository w/dirty subrepo, and cd into it
  $ mkrepo() {
  >     hg init $1
  >     cd $1
  >     hg qinit
  > }

fn to create dirty subrepo
  $ mksubrepo() {
  >     hg init $1
  >     cd $1
  >     echo a > a
  >     hg add
  >     cd ..
  > }

  $ testadd() {
  >     cat - > "$stdin"
  >     mksubrepo sub
  >     echo sub = sub >> .hgsub
  >     hg add .hgsub
  >     echo % abort when adding .hgsub w/dirty subrepo
  >     hg status -S
  >     echo '%' $*
  >     cat "$stdin" | hg $*
  >     echo [$?]
  >     hg -R sub ci -m0sub
  >     echo % update substate when adding .hgsub w/clean updated subrepo
  >     hg status -S
  >     echo '%' $*
  >     cat "$stdin" | hg $*
  >     hg debugsub
  > }

  $ testmod() {
  >     cat - > "$stdin"
  >     mksubrepo sub2
  >     echo sub2 = sub2 >> .hgsub
  >     echo % abort when modifying .hgsub w/dirty subrepo
  >     hg status -S
  >     echo '%' $*
  >     cat "$stdin" | hg $*
  >     echo [$?]
  >     hg -R sub2 ci -m0sub2
  >     echo % update substate when modifying .hgsub w/clean updated subrepo
  >     hg status -S
  >     echo '%' $*
  >     cat "$stdin" | hg $*
  >     hg debugsub
  > }

  $ testrm1() {
  >     cat - > "$stdin"
  >     mksubrepo sub3
  >     echo sub3 = sub3 >> .hgsub
  >     hg ci -Aqmsub3
  >     $EXTRA
  >     echo b >> sub3/a
  >     hg rm .hgsub
  >     echo % update substate when removing .hgsub w/dirty subrepo
  >     hg status -S
  >     echo '%' $*
  >     cat "$stdin" | hg $*
  >     echo % debugsub should be empty
  >     hg debugsub
  > }

  $ testrm2() {
  >     cat - > "$stdin"
  >     mksubrepo sub4
  >     echo sub4 = sub4 >> .hgsub
  >     hg ci -Aqmsub4
  >     $EXTRA
  >     hg rm .hgsub
  >     echo % update substate when removing .hgsub w/clean updated subrepo
  >     hg status -S
  >     echo '%' $*
  >     cat "$stdin" | hg $*
  >     echo % debugsub should be empty
  >     hg debugsub
  > }


handle subrepos safely on qnew

  $ mkrepo repo-2499-qnew
  $ testadd qnew -X path:no-effect -m0 0.diff
  adding a
  % abort when adding .hgsub w/dirty subrepo
  A .hgsub
  A sub/a
  % qnew -X path:no-effect -m0 0.diff
  abort: uncommitted changes in subrepository 'sub'
  [255]
  % update substate when adding .hgsub w/clean updated subrepo
  A .hgsub
  A sub/a
  % qnew -X path:no-effect -m0 0.diff
  path sub
   source   sub
   revision b2fdb12cd82b021c3b7053d67802e77b6eeaee31

  $ testmod qnew --cwd .. -R repo-2499-qnew -X path:no-effect -m1 1.diff
  adding a
  % abort when modifying .hgsub w/dirty subrepo
  M .hgsub
  A sub2/a
  % qnew --cwd .. -R repo-2499-qnew -X path:no-effect -m1 1.diff
  abort: uncommitted changes in subrepository 'sub2'
  [255]
  % update substate when modifying .hgsub w/clean updated subrepo
  M .hgsub
  A sub2/a
  % qnew --cwd .. -R repo-2499-qnew -X path:no-effect -m1 1.diff
  path sub
   source   sub
   revision b2fdb12cd82b021c3b7053d67802e77b6eeaee31
  path sub2
   source   sub2
   revision 1f94c7611cc6b74f5a17b16121a1170d44776845

  $ hg qpop -qa
  patch queue now empty
  $ testrm1 qnew -m2 2.diff
  adding a
  % update substate when removing .hgsub w/dirty subrepo
  M sub3/a
  R .hgsub
  % qnew -m2 2.diff
  % debugsub should be empty

  $ hg qpop -qa
  patch queue now empty
  $ testrm2 qnew -m3 3.diff
  adding a
  % update substate when removing .hgsub w/clean updated subrepo
  R .hgsub
  % qnew -m3 3.diff
  % debugsub should be empty

  $ cd ..


handle subrepos safely on qrefresh

  $ mkrepo repo-2499-qrefresh
  $ hg qnew -m0 0.diff
  $ testadd qrefresh
  adding a
  % abort when adding .hgsub w/dirty subrepo
  A .hgsub
  A sub/a
  % qrefresh
  abort: uncommitted changes in subrepository 'sub'
  [255]
  % update substate when adding .hgsub w/clean updated subrepo
  A .hgsub
  A sub/a
  % qrefresh
  path sub
   source   sub
   revision b2fdb12cd82b021c3b7053d67802e77b6eeaee31

  $ hg qnew -m1 1.diff
  $ testmod qrefresh
  adding a
  % abort when modifying .hgsub w/dirty subrepo
  M .hgsub
  A sub2/a
  % qrefresh
  abort: uncommitted changes in subrepository 'sub2'
  [255]
  % update substate when modifying .hgsub w/clean updated subrepo
  M .hgsub
  A sub2/a
  % qrefresh
  path sub
   source   sub
   revision b2fdb12cd82b021c3b7053d67802e77b6eeaee31
  path sub2
   source   sub2
   revision 1f94c7611cc6b74f5a17b16121a1170d44776845

  $ hg qpop -qa
  patch queue now empty
  $ EXTRA='hg qnew -m2 2.diff'
  $ testrm1 qrefresh
  adding a
  % update substate when removing .hgsub w/dirty subrepo
  M sub3/a
  R .hgsub
  % qrefresh
  % debugsub should be empty

  $ hg qpop -qa
  patch queue now empty
  $ EXTRA='hg qnew -m3 3.diff'
  $ testrm2 qrefresh
  adding a
  % update substate when removing .hgsub w/clean updated subrepo
  R .hgsub
  % qrefresh
  % debugsub should be empty
  $ EXTRA=

  $ cd ..


handle subrepos safely on qpush/qpop
(and we cannot qpop / qpush with a modified subrepo)

  $ mkrepo repo-2499-qpush
  $ mksubrepo sub
  adding a
  $ hg -R sub ci -m0sub
  $ echo sub = sub > .hgsub
  $ hg add .hgsub
  $ hg commit -m0
  $ hg debugsub
  path sub
   source   sub
   revision b2fdb12cd82b021c3b7053d67802e77b6eeaee31
  $ echo foo > ./sub/a
  $ hg -R sub commit -m foo
  $ hg commit -m1
  $ hg qimport -r "0:tip"
  $ hg -R sub id --id
  aa037b301eba

qpop
  $ hg -R sub update 0000
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg qpop
  abort: local changed subrepos found, qrefresh first
  [255]
  $ hg revert sub
  reverting subrepo sub
  adding sub/a (glob)
  $ hg qpop
  popping 1
  now at: 0
  $ hg status -AS
  C .hgsub
  C .hgsubstate
  C sub/a
  $ hg -R sub id --id
  b2fdb12cd82b

qpush
  $ hg -R sub update 0000
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg qpush
  abort: local changed subrepos found, qrefresh first
  [255]
  $ hg revert sub
  reverting subrepo sub
  adding sub/a (glob)
  $ hg qpush
  applying 1
   subrepository sub diverged (local revision: b2fdb12cd82b, remote revision: aa037b301eba)
  (M)erge, keep (l)ocal or keep (r)emote? m
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  now at: 1
  $ hg status -AS
  C .hgsub
  C .hgsubstate
  C sub/a
  $ hg -R sub id --id
  aa037b301eba

  $ cd ..


handle subrepos safely on qrecord

  $ mkrepo repo-2499-qrecord
  $ testadd qrecord --config ui.interactive=1 -m0 0.diff <<EOF
  > y
  > y
  > EOF
  adding a
  % abort when adding .hgsub w/dirty subrepo
  A .hgsub
  A sub/a
  % qrecord --config ui.interactive=1 -m0 0.diff
  diff --git a/.hgsub b/.hgsub
  new file mode 100644
  examine changes to '.hgsub'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +sub = sub
  record this change to '.hgsub'? [Ynesfdaq?] y
  
  warning: subrepo spec file '.hgsub' not found
  abort: uncommitted changes in subrepository 'sub'
  [255]
  % update substate when adding .hgsub w/clean updated subrepo
  A .hgsub
  A sub/a
  % qrecord --config ui.interactive=1 -m0 0.diff
  diff --git a/.hgsub b/.hgsub
  new file mode 100644
  examine changes to '.hgsub'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +sub = sub
  record this change to '.hgsub'? [Ynesfdaq?] y
  
  warning: subrepo spec file '.hgsub' not found
  path sub
   source   sub
   revision b2fdb12cd82b021c3b7053d67802e77b6eeaee31
  $ testmod qrecord --config ui.interactive=1 -m1 1.diff <<EOF
  > y
  > y
  > EOF
  adding a
  % abort when modifying .hgsub w/dirty subrepo
  M .hgsub
  A sub2/a
  % qrecord --config ui.interactive=1 -m1 1.diff
  diff --git a/.hgsub b/.hgsub
  1 hunks, 1 lines changed
  examine changes to '.hgsub'? [Ynesfdaq?] y
  
  @@ -1,1 +1,2 @@
   sub = sub
  +sub2 = sub2
  record this change to '.hgsub'? [Ynesfdaq?] y
  
  abort: uncommitted changes in subrepository 'sub2'
  [255]
  % update substate when modifying .hgsub w/clean updated subrepo
  M .hgsub
  A sub2/a
  % qrecord --config ui.interactive=1 -m1 1.diff
  diff --git a/.hgsub b/.hgsub
  1 hunks, 1 lines changed
  examine changes to '.hgsub'? [Ynesfdaq?] y
  
  @@ -1,1 +1,2 @@
   sub = sub
  +sub2 = sub2
  record this change to '.hgsub'? [Ynesfdaq?] y
  
  path sub
   source   sub
   revision b2fdb12cd82b021c3b7053d67802e77b6eeaee31
  path sub2
   source   sub2
   revision 1f94c7611cc6b74f5a17b16121a1170d44776845

  $ hg qpop -qa
  patch queue now empty
  $ testrm1 qrecord --config ui.interactive=1 -m2 2.diff <<EOF
  > y
  > y
  > EOF
  adding a
  % update substate when removing .hgsub w/dirty subrepo
  M sub3/a
  R .hgsub
  % qrecord --config ui.interactive=1 -m2 2.diff
  diff --git a/.hgsub b/.hgsub
  deleted file mode 100644
  examine changes to '.hgsub'? [Ynesfdaq?] y
  
  % debugsub should be empty

  $ hg qpop -qa
  patch queue now empty
  $ testrm2 qrecord --config ui.interactive=1 -m3 3.diff <<EOF
  > y
  > y
  > EOF
  adding a
  % update substate when removing .hgsub w/clean updated subrepo
  R .hgsub
  % qrecord --config ui.interactive=1 -m3 3.diff
  diff --git a/.hgsub b/.hgsub
  deleted file mode 100644
  examine changes to '.hgsub'? [Ynesfdaq?] y
  
  % debugsub should be empty

  $ cd ..


correctly handle subrepos with patch queues
  $ mkrepo repo-subrepo-with-queue
  $ mksubrepo sub
  adding a
  $ hg -R sub qnew sub0.diff
  $ echo sub = sub >> .hgsub
  $ hg add .hgsub
  $ hg qnew 0.diff

  $ cd ..

check whether MQ operations can import updated .hgsubstate correctly
both into 'revision' and 'patch file under .hg/patches':

  $ hg init importing-hgsubstate
  $ cd importing-hgsubstate

  $ echo a > a
  $ hg commit -u test -d '0 0' -Am '#0 in parent'
  adding a
  $ hg init sub
  $ echo sa > sub/sa
  $ hg -R sub commit -u test -d '0 0' -Am '#0 in sub'
  adding sa
  $ echo 'sub = sub' > .hgsub
  $ touch .hgsubstate
  $ hg add .hgsub .hgsubstate

  $ hg qnew -u test -d '0 0' import-at-qnew
  $ hg -R sub parents --template '{node} sub\n'
  b6f6e9c41f3dfd374a6d2ed4535c87951cf979cf sub
  $ cat .hgsubstate
  b6f6e9c41f3dfd374a6d2ed4535c87951cf979cf sub
  $ hg diff -c tip
  diff -r f499373e340c -r f69e96d86e75 .hgsub
  --- /dev/null
  +++ b/.hgsub
  @@ -0,0 +1,1 @@
  +sub = sub
  diff -r f499373e340c -r f69e96d86e75 .hgsubstate
  --- /dev/null
  +++ b/.hgsubstate
  @@ -0,0 +1,1 @@
  +b6f6e9c41f3dfd374a6d2ed4535c87951cf979cf sub
  $ cat .hg/patches/import-at-qnew
  # HG changeset patch
  # User test
  # Date 0 0
  # Parent  f499373e340cdca5d01dee904aeb42dd2a325e71
  
  diff -r f499373e340c -r f69e96d86e75 .hgsub
  --- /dev/null
  +++ b/.hgsub
  @@ -0,0 +1,1 @@
  +sub = sub
  diff -r f499373e340c -r f69e96d86e75 .hgsubstate
  --- /dev/null
  +++ b/.hgsubstate
  @@ -0,0 +1,1 @@
  +b6f6e9c41f3dfd374a6d2ed4535c87951cf979cf sub
  $ hg parents --template '{node}\n'
  f69e96d86e75a6d4fd88285dc9697acb23951041
  $ hg parents --template '{files}\n'
  .hgsub .hgsubstate

check also whether qnew not including ".hgsubstate" explicitly causes
as same result (in node hash) as one including it.

  $ hg qpop -a -q
  patch queue now empty
  $ hg qdelete import-at-qnew
  $ echo 'sub = sub' > .hgsub
  $ hg add .hgsub
  $ rm -f .hgsubstate
  $ hg qnew -u test -d '0 0' import-at-qnew
  $ hg parents --template '{node}\n'
  f69e96d86e75a6d4fd88285dc9697acb23951041
  $ hg parents --template '{files}\n'
  .hgsub .hgsubstate

check whether qrefresh imports updated .hgsubstate correctly

  $ hg qpop
  popping import-at-qnew
  patch queue now empty
  $ hg qpush
  applying import-at-qnew
  now at: import-at-qnew
  $ hg parents --template '{files}\n'
  .hgsub .hgsubstate

  $ hg qnew import-at-qrefresh
  $ echo sb > sub/sb
  $ hg -R sub commit -u test -d '0 0' -Am '#1 in sub'
  adding sb
  $ hg qrefresh -u test -d '0 0'
  $ hg -R sub parents --template '{node} sub\n'
  88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ cat .hgsubstate
  88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ hg diff -c tip
  diff -r 05b056bb9c8c -r d987bec230f4 .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -b6f6e9c41f3dfd374a6d2ed4535c87951cf979cf sub
  +88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ cat .hg/patches/import-at-qrefresh
  # HG changeset patch
  # User test
  # Date 0 0
  # Parent  05b056bb9c8c05ff15258b84fd42ab3527271033
  
  diff -r 05b056bb9c8c .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -b6f6e9c41f3dfd374a6d2ed4535c87951cf979cf sub
  +88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ hg parents --template '{files}\n'
  .hgsubstate

  $ hg qrefresh -u test -d '0 0'
  $ cat .hgsubstate
  88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ hg diff -c tip
  diff -r 05b056bb9c8c -r d987bec230f4 .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -b6f6e9c41f3dfd374a6d2ed4535c87951cf979cf sub
  +88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ cat .hg/patches/import-at-qrefresh
  # HG changeset patch
  # User test
  # Date 0 0
  # Parent  05b056bb9c8c05ff15258b84fd42ab3527271033
  
  diff -r 05b056bb9c8c .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -b6f6e9c41f3dfd374a6d2ed4535c87951cf979cf sub
  +88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ hg parents --template '{files}\n'
  .hgsubstate

  $ hg update -C tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg qpop -a
  popping import-at-qrefresh
  popping import-at-qnew
  patch queue now empty

  $ hg -R sub update -C 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'sub = sub' > .hgsub
  $ hg commit -Am '#1 in parent'
  adding .hgsub
  $ hg -R sub update -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg commit -Am '#2 in parent (but will be rolled back soon)'
  $ hg rollback
  repository tip rolled back to revision 1 (undo commit)
  working directory now based on revision 1
  $ hg status
  M .hgsubstate
  $ hg qnew -u test -d '0 0' checkstate-at-qnew
  $ hg -R sub parents --template '{node} sub\n'
  88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ cat .hgsubstate
  88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ hg diff -c tip
  diff -r 4d91eb2fa1d1 -r 1259c112d884 .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -b6f6e9c41f3dfd374a6d2ed4535c87951cf979cf sub
  +88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ cat .hg/patches/checkstate-at-qnew
  # HG changeset patch
  # User test
  # Date 0 0
  # Parent  4d91eb2fa1d1b22ec513347b9cd06f6b49d470fa
  
  diff -r 4d91eb2fa1d1 -r 1259c112d884 .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -b6f6e9c41f3dfd374a6d2ed4535c87951cf979cf sub
  +88ac1bef5ed43b689d1d200b59886b675dec474b sub
  $ hg parents --template '{files}\n'
  .hgsubstate

check whether qrefresh not including ".hgsubstate" explicitly causes
as same result (in node hash) as one including it.

  $ hg update -C -q 0
  $ hg qpop -a -q
  patch queue now empty
  $ hg qnew -u test -d '0 0' add-hgsub-at-qrefresh
  $ echo 'sub = sub' > .hgsub
  $ echo > .hgsubstate
  $ hg add .hgsub .hgsubstate
  $ hg qrefresh -u test -d '0 0'
  $ hg parents --template '{node}\n'
  7c48c35501aae6770ed9c2517014628615821a8e
  $ hg parents --template '{files}\n'
  .hgsub .hgsubstate

  $ hg qpop -a -q
  patch queue now empty
  $ hg qdelete add-hgsub-at-qrefresh
  $ hg qnew -u test -d '0 0' add-hgsub-at-qrefresh
  $ echo 'sub = sub' > .hgsub
  $ hg add .hgsub
  $ rm -f .hgsubstate
  $ hg qrefresh -u test -d '0 0'
  $ hg parents --template '{node}\n'
  7c48c35501aae6770ed9c2517014628615821a8e
  $ hg parents --template '{files}\n'
  .hgsub .hgsubstate

  $ cd ..

  $ cd ..
