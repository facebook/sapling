#chg-compatible

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
  $ hg root --config "configs.regen-command=touch $TESTTMP/touch" --config configs.generationtime=0 > /dev/null
  $ sleep 1
  $ ls $TESTTMP/touch
  $TESTTMP/touch


Only load config a single time.
  $ LOG=configloader::hg=info hg files abc
   INFO configloader::hg: loading config repo_path=$TESTTMP* (glob)
  [1]
