#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ readconfig <<EOF
  > [alias]
  > tlog = log --template "{node|short}: '{desc}'\n"
  > EOF

  $ hg init a
  $ cd a

  $ echo a > a
  $ hg ci -Aqm0

  $ echo foo >> a
  $ hg ci -Aqm1

  $ hg up -q 'desc(0)'

  $ echo bar >> a
  $ hg ci -qm2

  $ tglog
  @  a578af2cfd0c '2'
  │
  │ o  3560197d8331 '1'
  ├─╯
  o  f7b1eb17ad24 '0'
  

  $ cd ..

  $ hg clone -q a b

  $ cd b
  $ cat .hg/hgrc
  # example repository config (see 'hg help config' for more info)
  [paths]
  default = $TESTTMP/a
  
  # URL aliases to other repo sources
  # (see 'hg help config.paths' for more info)
  #
  # my-fork = https://example.com/jdoe/example-repo
  
  [ui]
  # name and email (local to this repository, optional), e.g.
  # username = Jane Doe <jdoe@example.com>

  $ echo red >> a
  $ hg ci -qm3

  $ hg up -q default

  $ echo blue >> a
  $ hg ci -qm4

  $ tglog
  @  acadbdc73b28 '4'
  │
  o  5de9cb7d8f67 '3'
  │
  o  a578af2cfd0c '2'
  │
  │ o  3560197d8331 '1'
  ├─╯
  o  f7b1eb17ad24 '0'
  

  $ hg tlog -r 'outgoing()'
  5de9cb7d8f67: '3'
  acadbdc73b28: '4'

  $ hg tlog -r 'outgoing("../a")'
  5de9cb7d8f67: '3'
  acadbdc73b28: '4'

  $ echo "green = ../a" >> .hg/hgrc

  $ cat .hg/hgrc
  # example repository config (see 'hg help config' for more info)
  [paths]
  default = $TESTTMP/a
  
  # URL aliases to other repo sources
  # (see 'hg help config.paths' for more info)
  #
  # my-fork = https://example.com/jdoe/example-repo
  
  [ui]
  # name and email (local to this repository, optional), e.g.
  # username = Jane Doe <jdoe@example.com>
  green = ../a

  $ hg tlog -r 'outgoing("green")'
  abort: repository green does not exist!
  [255]

  $ cd ..
