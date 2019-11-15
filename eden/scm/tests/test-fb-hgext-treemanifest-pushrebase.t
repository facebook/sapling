  $ setconfig treemanifest.flatcompat=False
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=

  $ . "$TESTDIR/library.sh"

  $ cat >> $TESTTMP/helper.sh <<EOF
  > initclients() {
  >     for i in {0..9} ; do
  >        hg init client\$i
  >        cat >> client\$i/.hg/hgrc <<EOF2
  > [paths]
  > default=ssh://user@dummy/master
  > [treemanifest]
  > sendtrees=True
  > EOF2
  >        hg -R client\$i pull -q
  >        hg -R client\$i up -q master
  >        echo >> client\$i/\$i
  >        hg -R client\$i commit -Aqm "add $i"
  >     done
  > }
  > pushclients() {
  >     for i in {0..9} ; do
  >        hg -R client\$i push --to master -B master >/dev/null 2>/dev/null &
  >     done
  >     wait
  > }
  > EOF
  $ . "$TESTTMP/helper.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > pushrebase=
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
  > treemanifest=
  > [treemanifest]
  > server=True
  > # Sleep gives all the hg serve processes time to load the original repo
  > # state. Otherwise there are race with loading hg server while pushes are
  > # happening.
  > [remotefilelog]
  > server=True
  > [hooks]
  > prepushrebase.sleep=sleep 1
  > EOF
  $ mkdir subdir/
  $ touch subdir/a && hg ci -Aqm subdir/a
  $ hg book master
  $ cd ..

  $ initclients
  fetching tree '' b25ede39b5168327f0596fcf74d1ff53842bad2a
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
  subdir\x008515d4bfda768e04af4c13a69a72e28c7effbea7t (esc)

Test that pushrebase hooks can access the commit data
  $ cat >> $TESTTMP/cathook.sh <<EOF
  > #! /bin/sh
  > echo "\$(hg cat -r \$HG_NODE subdir/a)"
  > exit 1
  > EOF
  $ chmod a+x $TESTTMP/cathook.sh
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > prepushrebase.cat=$TESTTMP/cathook.sh
  > EOF
  $ cd ..

  $ hg clone -q ssh://user@dummy/master hook_client
  fetching tree '' *, based on b25ede39b5168327f0596fcf74d1ff53842bad2a, found via * (glob)
  1 trees fetched over * (glob)
  $ cd hook_client
  $ hg up -q master
  $ echo baz >> subdir/a
  $ hg commit -Aqm 'hook commit'

- Push without sendtrees
  $ hg push --to master -B master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: abort: "unable to find the following nodes locally or on the server: ('', *)" (glob)
  remote: 
  remote: prepushrebase.cat hook exited with status 1
  abort: push failed on remote
  [255]

- Push with sendtrees
  $ hg push --to master -B master --config treemanifest.sendtrees=True
  pushing to ssh://user@dummy/master
  searching for changes
  remote: baz
  remote: prepushrebase.cat hook exited with status 1
  abort: push failed on remote
  [255]

- Disable the hook
  $ cat >> ../master/.hg/hgrc <<EOF
  > [hooks]
  > prepushrebase.cat=true
  > EOF

Push an empty commit with no trees
  $ hg up -q '.^'
  $ hg commit --config ui.allowemptycommit=True -m "Empty commit"
  $ hg push --to master --rev . --config treemanifest.sendtrees=True
  pushing to ssh://user@dummy/master
  searching for changes
  fetching tree '' *, based on *, found via * (glob)
  1 trees fetched over * (glob)
  remote: pushing 1 changeset:
  remote:     *  Empty commit (glob)
