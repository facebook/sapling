  $ . $TESTDIR/library.sh

setup configuration
  $ setup_config_repo
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF


setup repo

  $ hg init repo-hg

Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > remotefilelog=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

  $ touch a
  $ hg add a
  $ hg ci -ma
  $ hg log
  changeset:   0:3903775176ed
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)
  $ hg log -r. -T '{node}\n'
  3903775176ed42b1458a6281db4a0ccf4d9f287a

blobimport

  $ cd ..
  $ blobimport rocksdb repo-hg/.hg repo

smoke test to ensure bonsai_verify works

  $ bonsai_verify repo 3903775176ed42b1458a6281db4a0ccf4d9f287a
  * Ignoring setSSLLockTypes after initialization (glob)
  summary:  (re)
   * INFO 100.00% valid, total: 1, valid: 1, errors: 0, ignored: 0 (glob)
