#chg-compatible

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=
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

  $ hg log -T '{rev} ' -r '0:2' --config tweakdefaults.singlecolonwarn=1
  warning: use of ':' is deprecated
  0 1 2  (no-eol)
  $ hg log -T '{rev} ' -r '0:2'
  0 1 2  (no-eol)
  $ hg log -T '{rev} ' -r ':2' --config tweakdefaults.singlecolonwarn=1
  warning: use of ':' is deprecated
  0 1 2  (no-eol)
  $ hg log -T '{rev} ' -r ':2'
  0 1 2  (no-eol)
  $ hg log -T '{rev} ' -r '0:' --config tweakdefaults.singlecolonwarn=1
  warning: use of ':' is deprecated
  0 1 2  (no-eol)
  $ hg log -T '{rev} ' -r '0:'
  0 1 2  (no-eol)

In this testcase warning should not be shown
  $ hg log -T '{rev} ' -r ':' --config tweakdefaults.singlecolonwarn=1
  0 1 2  (no-eol)

Check that the custom message can be used
  $ hg log -T '{rev} ' -r '0:' --config tweakdefaults.singlecolonwarn=1 --config tweakdefaults.singlecolonmsg="hey stop that"
  warning: hey stop that
  0 1 2  (no-eol)

Check that we can abort as well
  $ hg log -T '{rev} ' -r '0:' --config tweakdefaults.singlecolonabort=1
  abort: use of ':' is deprecated
  [255]
  $ hg log -T '{rev} ' -r '0:' --config tweakdefaults.singlecolonabort=1 --config tweakdefaults.singlecolonmsg="no more colons"
  abort: no more colons
  [255]
