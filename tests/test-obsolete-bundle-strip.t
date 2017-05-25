==================================================
Test obsmarkers interaction with bundle and strip
==================================================

In practice, this file does not yet contains any tests for bundle and strip.
But their will be some soon (tm).

For now this test check the logic computing markers relevant to a set of
revision. That logic will be use by "hg bundle" to select the markers to
include, and strip to find the markers to backup.

Setup a repository with various case
====================================

Config setup
------------

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > # simpler log output
  > logtemplate = "{node|short}: {desc}\n"
  > 
  > [experimental]
  > # enable evolution
  > evolution = all
  > 
  > # include obsmarkers in bundle
  > evolution.bundle-obsmarker = yes
  > 
  > [extensions]
  > # needed for some tests
  > strip =
  > [defaults]
  > # we'll query many hidden changeset
  > debugobsolete = --hidden
  > EOF

  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }

  $ getid() {
  >    hg log --hidden --template '{node}\n' --rev "$1"
  > }

  $ mktestrepo () {
  >     [ -n "$1" ] || exit 1
  >     cd $TESTTMP
  >     hg init $1
  >     cd $1
  >     mkcommit ROOT
  > }

root setup
-------------

simple chain
============

.    A0
.   ⇠ø⇠◔ A1
.    |/
.    ●

setup
-----

  $ mktestrepo simple-chain
  $ mkcommit 'C-A0'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit 'C-A1'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A0")'`
  $ hg debugobsolete `getid 'desc("C-A0")'` a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1
  $ hg debugobsolete a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 `getid 'desc("C-A1")'`

  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  cf2c22470d67: C-A1
  |
  | x  84fcb0dfe17b: C-A0
  |/
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ hg debugobsolete --rev 'desc("C-A0")'
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ hg debugobsolete --rev 'desc("C-A1")'
  84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

chain with prune children
=========================

.  ⇠⊗ B0
.   |
.  ⇠ø⇠◔ A1
.     |
.     ●

setup
-----

  $ mktestrepo prune
  $ mkcommit 'C-A0'
  $ mkcommit 'C-B0'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit 'C-A1'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A0")'`
  $ hg debugobsolete `getid 'desc("C-A0")'` `getid 'desc("C-A1")'`
  $ hg debugobsolete --record-parents `getid 'desc("C-B0")'`
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  cf2c22470d67: C-A1
  |
  | x  29f93b1df87b: C-B0
  | |
  | x  84fcb0dfe17b: C-A0
  |/
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ hg debugobsolete --rev 'desc("C-A0")'
  29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ hg debugobsolete --rev 'desc("C-B0")'
  29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ hg debugobsolete --rev 'desc("C-A1")'
  29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

chain with precursors also pruned
=================================

.   A0 (also pruned)
.  ⇠ø⇠◔ A1
.     |
.     ●

setup
-----

  $ mktestrepo prune-inline
  $ mkcommit 'C-A0'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit 'C-A1'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A0")'`
  $ hg debugobsolete --record-parents `getid 'desc("C-A0")'`
  $ hg debugobsolete `getid 'desc("C-A0")'` `getid 'desc("C-A1")'`
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  cf2c22470d67: C-A1
  |
  | x  84fcb0dfe17b: C-A0
  |/
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ hg debugobsolete --rev 'desc("C-A0")'
  84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ hg debugobsolete --rev 'desc("C-A1")'
  84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

chain with missing prune
========================

.   ⊗ B
.   |
.  ⇠◌⇠◔ A1
.   |
.   ●

setup
-----

  $ mktestrepo missing-prune
  $ mkcommit 'C-A0'
  $ mkcommit 'C-B0'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit 'C-A1'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A0")'`
  $ hg debugobsolete `getid 'desc("C-A0")'` `getid 'desc("C-A1")'`
  $ hg debugobsolete --record-parents `getid 'desc("C-B0")'`

(it is annoying to create prune with parent data without the changeset, so we strip it after the fact)

  $ hg strip --hidden --rev 'desc("C-A0")::' --no-backup

  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  cf2c22470d67: C-A1
  |
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ hg debugobsolete --rev 'desc("C-A1")'
  29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

chain with precursors also pruned
=================================

.   A0 (also pruned)
.  ⇠◌⇠◔ A1
.     |
.     ●

setup
-----

  $ mktestrepo prune-inline-missing
  $ mkcommit 'C-A0'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit 'C-A1'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A0")'`
  $ hg debugobsolete --record-parents `getid 'desc("C-A0")'`
  $ hg debugobsolete `getid 'desc("C-A0")'` `getid 'desc("C-A1")'`

(it is annoying to create prune with parent data without the changeset, so we strip it after the fact)

  $ hg strip --hidden --rev 'desc("C-A0")::' --no-backup

  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  cf2c22470d67: C-A1
  |
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ hg debugobsolete --rev 'desc("C-A1")'
  84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
