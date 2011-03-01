  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "record=" >> $HGRCPATH
  $ echo "[diff]" >> $HGRCPATH
  $ echo "nodates=1" >> $HGRCPATH

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
  $ testadd qnew -m0 0.diff
  adding a
  % abort when adding .hgsub w/dirty subrepo
  A .hgsub
  A sub/a
  % qnew -m0 0.diff
  abort: uncommitted changes in subrepository sub
  [255]
  % update substate when adding .hgsub w/clean updated subrepo
  A .hgsub
  % qnew -m0 0.diff
  committing subrepository sub
  path sub
   source   sub
   revision b2fdb12cd82b021c3b7053d67802e77b6eeaee31

  $ testmod qnew -m1 1.diff
  adding a
  % abort when modifying .hgsub w/dirty subrepo
  M .hgsub
  A sub2/a
  % qnew -m1 1.diff
  abort: uncommitted changes in subrepository sub2
  [255]
  % update substate when modifying .hgsub w/clean updated subrepo
  M .hgsub
  % qnew -m1 1.diff
  committing subrepository sub2
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
  abort: uncommitted changes in subrepository sub
  [255]
  % update substate when adding .hgsub w/clean updated subrepo
  A .hgsub
  % qrefresh
  committing subrepository sub
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
  abort: uncommitted changes in subrepository sub2
  [255]
  % update substate when modifying .hgsub w/clean updated subrepo
  M .hgsub
  % qrefresh
  committing subrepository sub2
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

  $ mkrepo repo-2499-qpush
  $ mksubrepo sub
  adding a
  $ hg -R sub ci -m0sub
  $ echo sub = sub > .hgsub
  $ hg add .hgsub
  $ hg qnew -m0 0.diff
  committing subrepository sub
  $ hg debugsub
  path sub
   source   sub
   revision b2fdb12cd82b021c3b7053d67802e77b6eeaee31

qpop
  $ hg qpop
  popping 0.diff
  patch queue now empty
  $ hg status -AS
  $ hg debugsub

qpush
  $ hg qpush
  applying 0.diff
  now at: 0.diff
  $ hg status -AS
  C .hgsub
  C .hgsubstate
  C sub/a
  $ hg debugsub
  path sub
   source   sub
   revision b2fdb12cd82b021c3b7053d67802e77b6eeaee31

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
  examine changes to '.hgsub'? [Ynsfdaq?] 
  abort: uncommitted changes in subrepository sub
  [255]
  % update substate when adding .hgsub w/clean updated subrepo
  A .hgsub
  % qrecord --config ui.interactive=1 -m0 0.diff
  diff --git a/.hgsub b/.hgsub
  new file mode 100644
  examine changes to '.hgsub'? [Ynsfdaq?] 
  committing subrepository sub
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
  examine changes to '.hgsub'? [Ynsfdaq?] 
  @@ -1,1 +1,2 @@
   sub = sub
  +sub2 = sub2
  record this change to '.hgsub'? [Ynsfdaq?] 
  abort: uncommitted changes in subrepository sub2
  [255]
  % update substate when modifying .hgsub w/clean updated subrepo
  M .hgsub
  % qrecord --config ui.interactive=1 -m1 1.diff
  diff --git a/.hgsub b/.hgsub
  1 hunks, 1 lines changed
  examine changes to '.hgsub'? [Ynsfdaq?] 
  @@ -1,1 +1,2 @@
   sub = sub
  +sub2 = sub2
  record this change to '.hgsub'? [Ynsfdaq?] 
  committing subrepository sub2
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
  examine changes to '.hgsub'? [Ynsfdaq?] 
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
  examine changes to '.hgsub'? [Ynsfdaq?] 
  % debugsub should be empty

  $ cd ..
