  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ . "$TESTDIR/library.sh"

  $ cat >> $TESTTMP/helper.sh <<EOF
  > initclients() {
  >     for i in {0..9} ; do
  >        hg init client\$i
  >        cat >> client\$i/.hg/hgrc <<EOF2
  > [paths]
  > default=ssh://user@dummy/master
  > [extensions]
  > fastmanifest=$TESTDIR/../fastmanifest
  > treemanifest=$TESTDIR/../treemanifest
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > EOF2
  > 
  >        hg -R client\$i pull -q
  >        hg -R client\$i up -q master
  >        echo >> client\$i/\$i
  >        hg -R client\$i commit -Aqm "add $i"
  >     done
  > }
  > 
  > pushclients() {
  >     for i in {0..9} ; do
  >        hg -R client\$i push --to master -B master 2>&1 >/dev/null &
  >     done
  >     wait
  > }
  > EOF
  $ . "$TESTTMP/helper.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > bundle2hooks=$TESTDIR/../hgext3rd/bundle2hooks.py
  > pushrebase=$TESTDIR/../hgext3rd/pushrebase.py
  > [experimental]
  > bundle2lazylocking=True
  > [remotefilelog]
  > reponame=master
  > EOF

Test that multiple fighting pushes result in the correct flat and tree manifests

  $ hg init master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../treemanifest
  > [treemanifest]
  > server=True
  > # Sleep gives all the hg serve processes time to load the original repo
  > # state. Otherwise there are race with loading hg server while pushes are
  > # happening.
  > [hooks]
  > prepushrebase.sleep=sleep 0.2
  > EOF
  $ mkdir subdir/
  $ touch subdir/a && hg ci -Aqm subdir/a
  $ hg book master
  $ hg backfilltree
  $ cd ..

  $ initclients
  2 trees fetched over * (glob)
  $ pushclients

  $ cd master
  $ hg debugdata .hg/store/00manifesttree.i 10
  0\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  1\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  2\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  3\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  4\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  5\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  6\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  7\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  8\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  9\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  subdir\x008515d4bfda768e04af4c13a69a72e28c7effbea7t (esc)
  $ hg debugdata -m 10
  0\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  1\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  2\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  3\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  4\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  5\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  6\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  7\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  8\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  9\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)
  subdir/a\x00b80de5d138758541c5f05265ad144ab9fa86d1db (esc)
