  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > 
  > [alias]
  > tlog = log --template "{rev}:{node|short}: '{desc}' {branches}\n"
  > tglog = tlog -G
  > tout = out --template "{rev}:{node|short}: '{desc}' {branches}\n"
  > EOF

  $ hg init a
  $ cd a

  $ echo a > a
  $ hg ci -Aqm0

  $ echo foo >> a
  $ hg ci -Aqm1

  $ hg up -q 0

  $ hg branch stable
  marked working directory as branch stable
  $ echo bar >> a
  $ hg ci -qm2

  $ hg tglog
  @  2:7bee6c3bea3a: '2' stable
  |
  | o  1:3560197d8331: '1'
  |/
  o  0:f7b1eb17ad24: '0'
  

  $ cd ..

  $ hg clone -q a#stable b

  $ cd b
  $ cat .hg/hgrc
  [paths]
  default = */a#stable (glob)

  $ echo red >> a
  $ hg ci -qm3

  $ hg up -q default

  $ echo blue >> a
  $ hg ci -qm4

  $ hg tglog
  @  3:f0461977a3db: '4'
  |
  | o  2:1d4099801a4e: '3' stable
  | |
  | o  1:7bee6c3bea3a: '2' stable
  |/
  o  0:f7b1eb17ad24: '0'
  

  $ hg tout
  comparing with */a (glob)
  searching for changes
  2:1d4099801a4e: '3' stable

  $ hg tlog -r 'outgoing()'
  2:1d4099801a4e: '3' stable

  $ hg tout ../a#default
  comparing with ../a
  searching for changes
  3:f0461977a3db: '4' 

  $ hg tlog -r 'outgoing("../a#default")'
  3:f0461977a3db: '4' 

  $ echo "green = ../a#default" >> .hg/hgrc

  $ cat .hg/hgrc
  [paths]
  default = */a#stable (glob)
  green = ../a#default

  $ hg tout green
  comparing with */a (glob)
  searching for changes
  3:f0461977a3db: '4' 

  $ hg tlog -r 'outgoing("green")'
  3:f0461977a3db: '4' 

