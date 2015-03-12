# -*- coding: utf-8 -*-

Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo

utf-8 encoded commit message
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add älphà'

Create some commits using latin1 encoding
The warning message changed in Git 1.8.0
  $ . $TESTDIR/latin-1-encoding
  Warning: commit message (did|does) not conform to UTF-8. (re)
  You may want to amend it after fixing the message, or set the config
  variable i18n.commitencoding to the encoding your project uses.
  Warning: commit message (did|does) not conform to UTF-8. (re)
  You may want to amend it after fixing the message, or set the config
  variable i18n.commitencoding to the encoding your project uses.

  $ cd ..
  $ git init --bare gitrepo2
  Initialized empty Git repository in $TESTTMP/gitrepo2/

  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo

  $ HGENCODING=utf-8 hg log --graph --debug | grep -v 'phase:' | grep -v ': *author=' | grep -v ': *message='
  @  changeset:   3:b8a0ac52f339ccd6d5729508bac4aee6e8b489d8
  |  bookmark:    master
  |  tag:         default/master
  |  tag:         tip
  |  parent:      2:8bc4d64940260d4a1e70b54c099d3a76c83ff41e
  |  parent:      -1:0000000000000000000000000000000000000000
  |  manifest:    3:ea49f93388380ead5601c8fcbfa187516e7c2ed8
  |  user:        tést èncödîng <test@example.org>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  files+:      delta
  |  extra:       branch=default
  |  extra:       committer=test <test@example.org> 1167609613 0
  |  extra:       encoding=latin-1
  |  extra:       hg-git-rename-source=git
  |  description:
  |  add d\xc3\xa9lt\xc3\xa0 (esc)
  |
  |
  o  changeset:   2:8bc4d64940260d4a1e70b54c099d3a76c83ff41e
  |  parent:      1:f35a3100b78e57a0f5e4589a438f952a14b26ade
  |  parent:      1:(9f6268bfc9eb3956c5ab8752d7b983b0ffe57115|955b24cf6f8f293741d3f39110c6fe554c292533) (re)
  |  manifest:    2:f580e7da3673c137370da2b931a1dee83590d7b4
  |  user:        t\xc3\xa9st \xc3\xa8nc\xc3\xb6d\xc3\xaeng <test@example.org> (esc)
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  files+:      gamma
  |  extra:       branch=default
  |  extra:       committer=test <test@example.org> 1167609612 0
  |  extra:       hg-git-rename-source=git
  |  description:
  |  add g\xc3\xa4mm\xc3\xa2 (esc)
  |
  |
  o  changeset:   1:f35a3100b78e57a0f5e4589a438f952a14b26ade
  |  parent:      0:87cd29b67a9159eec3b5495b0496ef717b2769f5
  |  parent:      -1:0000000000000000000000000000000000000000
  |  manifest:    1:f0bd6fbafbaebe4bb59c35108428f6fce152431d
  |  user:        t\xc3\xa9st \xc3\xa8nc\xc3\xb6d\xc3\xaeng <test@example.org> (esc)
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  files+:      beta
  |  extra:       branch=default
  |  extra:       committer=test <test@example.org> 1167609611 0
  |  extra:       hg-git-rename-source=git
  |  description:
  |  add beta
  |
  |
  o  changeset:   0:87cd29b67a9159eec3b5495b0496ef717b2769f5
     parent:      -1:0000000000000000000000000000000000000000
     parent:      -1:0000000000000000000000000000000000000000
     manifest:    0:8b8a0e87dfd7a0706c0524afa8ba67e20544cbf0
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     files+:      alpha
     extra:       branch=default
     extra:       hg-git-rename-source=git
     description:
     add \xc3\xa4lph\xc3\xa0 (esc)
  
  

  $ hg gclear
  clearing out the git cache data
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  searching for changes
  adding objects
  added 4 commits with 4 trees and 4 blobs

  $ cd ..
  $ git --git-dir=gitrepo2 log --pretty=medium
  commit e85fef6b515d555e45124a5dc39a019cf8db9ff0
  Author: t\xe9st \xe8nc\xf6d\xeeng <test@example.org> (esc)
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      add d\xe9lt\xe0 (esc)
  
  commit bd576458238cbda49ffcfbafef5242e103f1bc24
  Author: * <test@example.org> (glob)
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      add g*mm* (glob)
  
  commit 7a7e86fc1b24db03109c9fe5da28b352de59ce90
  Author: * <test@example.org> (glob)
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 0530b75d8c203e10dc934292a6a4032c6e958a83
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add älphà
