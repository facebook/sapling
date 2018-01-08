#require p4

  $ . $TESTDIR/p4setup.sh

Make a git-fusion-user
  $ p4 user -o -f git-fusion-user > /dev/null 2>&1

Submit change as actual user
  $ mkdir Main
  $ echo a > Main/a
  $ p4 add Main/a
  //depot/Main/a#1 - opened for add
  $ p4 submit -d initial_actual_user_submit
  Submitting change 1.
  Locking 1 files ...
  add //depot/Main/a#1
  Change 1 submitted.

Submit change as actual user
  $ p4 edit Main/a
  //depot/Main/a#1 - opened for edit
  $ echo a >> Main/a
  $ p4 submit -d second_actual_user_submit
  Submitting change 2.
  Locking 1 files ...
  edit //depot/Main/a#2
  Change 2 submitted.

Submit change as git-fusion-user
  $ echo b > Main/b
  $ p4 -u git-fusion-user add Main/b
  //depot/Main/b#1 - opened for add
  $ p4 -u git-fusion-user submit -d gf_submit
  Submitting change 3.
  Locking 1 files ...
  add //depot/Main/b#1
  Change 3 submitted.

Submit change as actual user after git-fusion-user
  $ echo c > Main/c
  $ p4 add Main/c
  //depot/Main/c#1 - opened for add
  $ p4 submit -d actual_user_submit_after_gf
  Submitting change 4.
  Locking 1 files ...
  add //depot/Main/c#1
  Change 4 submitted.

Simple import
  $ cd $hgwd
  $ echo [p4fastimport] >> $HGRCPATH
  $ echo ignore-user='git-fusion-user' >> $HGRCPATH
  $ echo ignore-time-delta=30 >> $HGRCPATH
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --bookmark master --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  2 changelists to import.
  loading list of files.
  1 files to import.
  reading filelog //depot/Main/a
  importing repository.
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  writing filelog: a80d06849b33, p1 b789fdd96dc2, linkrev 1, 4 bytes, src: *, path: Main/a (glob)
  changelist 1: writing manifest. node: f495e209f723 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial_actual_user_submit
  changelist 2: writing manifest. node: c2c89b7ec832 p1: f495e209f723 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second_actual_user_submit
  writing bookmark
  updating the branch cache
  2 revision(s), 1 file(s) imported.

End Test

  stopping the p4 server
