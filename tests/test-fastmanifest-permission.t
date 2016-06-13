Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

Check diagnosis, debugging information
1) Setup configuration
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    hg ci -l msg
  > }

2) Set up the repo

  $ mkdir cachetesting
  $ cd cachetesting
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > fastmanifest=
  > [fastmanifest]
  > cachecutoffdays=-1
  > randomorder=False
  > EOF

  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ mkcommit e
  $ deauthorize() {
  >     chmod 100 .hg
  > }

  $ authorize() {
  >     chmod 755 .hg
  > }
  $ deauthorize
  $ hg debugcachemanifest -a
  warning: not using fastmanifest
  (make sure that .hg/store is writeable)
  $ authorize
  $ hg debugcachemanifest -a

