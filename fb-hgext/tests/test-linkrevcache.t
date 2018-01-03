  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > linkrevcache=$TESTDIR/../hgext3rd/linkrevcache.py
  > EOF

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg ci -A a -m a
  $ echo 1 >> a
  $ hg ci -A a -m a1
  $ hg up '.^' -q
  $ hg graft --log 1 -q
  $ hg log -G -T '{rev}:{node} {desc}\n'
  @  2:e048e956c6a8c0f6108497df043989578ad97cc2 a1
  |  (grafted from da7a5140a61110d9ec1a678a11e796a71638dd6f)
  | o  1:da7a5140a61110d9ec1a678a11e796a71638dd6f a1
  |/
  o  0:3903775176ed42b1458a6281db4a0ccf4d9f287a a
  
  $ hg debugbuildlinkrevcache --debug
  a@d0c79e1d33097a72f79cb2e5a81c685e8f688d45: new linkrev 2
  $ hg debugverifylinkrevcache
  1 entries verified

  $ hg annotate a -r 1
  1: 1
  $ hg annotate a -r 2
  2: 1
