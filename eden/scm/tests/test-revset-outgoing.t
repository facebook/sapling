#chg-compatible

  $ disable treemanifest
  $ readconfig <<EOF
  > [alias]
  > tlog = log --template "{rev}:{node|short}: '{desc}'\n"
  > tout = out --template "{rev}:{node|short}: '{desc}'\n"
  > EOF

  $ hg init a
  $ cd a

  $ echo a > a
  $ hg ci -Aqm0

  $ echo foo >> a
  $ hg ci -Aqm1

  $ hg up -q 0

  $ echo bar >> a
  $ hg ci -qm2

  $ tglog
  @  2: a578af2cfd0c '2'
  |
  | o  1: 3560197d8331 '1'
  |/
  o  0: f7b1eb17ad24 '0'
  

  $ cd ..

  $ hg clone -q a b

  $ cd b
  $ cat .hg/hgrc
  # example repository config (see 'hg help config' for more info)
  [paths]
  default = $TESTTMP/a
  
  # path aliases to other clones of this repo in URLs or filesystem paths
  # (see 'hg help config.paths' for more info)
  #
  # default:pushurl = ssh://jdoe@example.net/hg/jdoes-fork
  # my-fork         = ssh://jdoe@example.net/hg/jdoes-fork
  # my-clone        = /home/jdoe/jdoes-clone
  
  [ui]
  # name and email (local to this repository, optional), e.g.
  # username = Jane Doe <jdoe@example.com>

  $ echo red >> a
  $ hg ci -qm3

  $ hg up -q default

  $ echo blue >> a
  $ hg ci -qm4

  $ tglog
  @  4: acadbdc73b28 '4'
  |
  o  3: 5de9cb7d8f67 '3'
  |
  o  2: a578af2cfd0c '2'
  |
  | o  1: 3560197d8331 '1'
  |/
  o  0: f7b1eb17ad24 '0'
  

  $ hg tout
  comparing with $TESTTMP/a
  searching for changes
  3:5de9cb7d8f67: '3'
  4:acadbdc73b28: '4'

  $ hg tlog -r 'outgoing()'
  3:5de9cb7d8f67: '3'
  4:acadbdc73b28: '4'

  $ hg tout ../a
  comparing with ../a
  searching for changes
  3:5de9cb7d8f67: '3'
  4:acadbdc73b28: '4'

  $ hg tlog -r 'outgoing("../a")'
  3:5de9cb7d8f67: '3'
  4:acadbdc73b28: '4'

  $ echo "green = ../a" >> .hg/hgrc

  $ cat .hg/hgrc
  # example repository config (see 'hg help config' for more info)
  [paths]
  default = $TESTTMP/a
  
  # path aliases to other clones of this repo in URLs or filesystem paths
  # (see 'hg help config.paths' for more info)
  #
  # default:pushurl = ssh://jdoe@example.net/hg/jdoes-fork
  # my-fork         = ssh://jdoe@example.net/hg/jdoes-fork
  # my-clone        = /home/jdoe/jdoes-clone
  
  [ui]
  # name and email (local to this repository, optional), e.g.
  # username = Jane Doe <jdoe@example.com>
  green = ../a

  $ hg tout green
  abort: repository green does not exist!
  [255]

  $ hg tlog -r 'outgoing("green")'
  abort: repository green does not exist!
  [255]

  $ cd ..
