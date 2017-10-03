  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=$TESTDIR/../hgext3rd/tweakdefaults.py
  > EOF

Setup repo

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg commit -Aqm a
  $ mkdir dir
  $ touch dir/b
  $ hg commit -Aqm b
  $ hg up -q 0
  $ echo x >> a
  $ hg commit -Aqm a2

Test that warning is shown whenever ':' is used with singlecolonwarn set

  $ hg log -G -T '{rev} {bookmarks}' -r '0:2' --config tweakdefaults.singlecolonwarn=1
  warning: use of ':' is deprecated
  @  2
  |
  | o  1
  |/
  o  0
  
  $ hg log -G -T '{rev} {bookmarks}' -r '0:2'
  @  2
  |
  | o  1
  |/
  o  0
  
  $ hg log -G -T '{rev} {bookmarks}' -r ':2' --config tweakdefaults.singlecolonwarn=1
  warning: use of ':' is deprecated
  @  2
  |
  | o  1
  |/
  o  0
  
  $ hg log -G -T '{rev} {bookmarks}' -r ':2'
  @  2
  |
  | o  1
  |/
  o  0
  
  $ hg log -G -T '{rev} {bookmarks}' -r '0:' --config tweakdefaults.singlecolonwarn=1
  warning: use of ':' is deprecated
  @  2
  |
  | o  1
  |/
  o  0
  
  $ hg log -G -T '{rev} {bookmarks}' -r '0:'
  @  2
  |
  | o  1
  |/
  o  0
  

In this testcase warning should not be shown
  $ hg log -G -T '{rev} {bookmarks}' -r ':' --config tweakdefaults.singlecolonwarn=1
  @  2
  |
  | o  1
  |/
  o  0
  
Check that the custom message can be used
  $ hg log -G -T '{rev} {bookmarks}' -r '0:' --config tweakdefaults.singlecolonwarn=1 --config tweakdefaults.singlecolonmsg="hey stop that"
  hey stop that
  @  2
  |
  | o  1
  |/
  o  0
  
