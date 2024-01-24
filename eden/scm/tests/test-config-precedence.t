#debugruntest-compatible

  $ configure modernclient

  $ newclientrepo test1

Sample config item that has been moved from configitems.py to Rust HG_PY_CORE_CONFIG
  $ hg config ui.timeout
  600

  $ hg config ui.timeout --config ui.timeout=123
  123

  $ cat > $TESTTMP/test.rc <<EOF
  > [ui]
  > timeout=456
  > EOF
  $ hg config ui.timeout --config ui.timeout=123 --configfile $TESTTMP/test.rc
  123

  $ hg config ui.timeout --configfile $TESTTMP/test.rc
  456

  $ cat >> .hg/hgrc <<EOF
  > [ui]
  > timeout=789
  > EOF
  $ hg config ui.timeout --configfile $TESTTMP/test.rc
  456

  $ hg config ui.timeout
  789

Make sure --config options are available when loading config itself.
"root" is not material - the important thing is that the regen-command is respected:

  $ echo > "$TESTTMP/test_hgrc"
  $ HG_TEST_INTERNALCONFIG="$TESTTMP/test_hgrc" LOG=configloader::hg=debug hg root --config "configs.regen-command=false" --config configs.generationtime=0 2>&1 | grep '^DEBUG.* spawn '
  DEBUG configloader::hg: spawn ["false"] because * (glob)

Only load config a single time.
  $ LOG=configloader::hg=info hg files abc
   INFO configloader::hg: loading config repo_path=$TESTTMP* (glob)
  [1]
