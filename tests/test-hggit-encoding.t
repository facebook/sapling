# -*- coding: utf-8 -*-

Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo

utf-8 encoded commit message
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add älphà'

Create some commits using latin1 encoding
The warning message changed in Git 1.8.0
  $ . $TESTDIR/hggit/latin-1-encoding
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
  @  changeset:   3:3c284d9743de7c02ac66b8b5ce10d39efd38d7bc
  |  bookmark:    master
  |  tag:         default/master
  |  tag:         tip
  |  parent:      2:727e37c486803fce561d97a80721324febade37e
  |  parent:      -1:0000000000000000000000000000000000000000
  |  manifest:    ea49f93388380ead5601c8fcbfa187516e7c2ed8
  |  user:        tést èncödîng <test@example.org>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  files+:      delta
  |  extra:       branch=default
  |  extra:       committer=test <test@example.org> 1167609613 0
  |  extra:       convert_revision=51c509c1c7eeb8f0a5b20aa3e894e8823f39171f
  |  extra:       encoding=latin-1
  |  extra:       hg-git-rename-source=git
  |  description:
  |  add d\xc3\xa9lt\xc3\xa0 (esc)
  |
  |
  o  changeset:   2:727e37c486803fce561d97a80721324febade37e
  |  parent:      1:5408f831a4d1a1d6ecccdddbe04c5a8b888a33c1
  |  parent:      -1:0000000000000000000000000000000000000000
  |  manifest:    f580e7da3673c137370da2b931a1dee83590d7b4
  |  user:        t\xc3\xa9st \xc3\xa8nc\xc3\xb6d\xc3\xaeng <test@example.org> (esc)
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  files+:      gamma
  |  extra:       branch=default
  |  extra:       committer=test <test@example.org> 1167609612 0
  |  extra:       convert_revision=bd576458238cbda49ffcfbafef5242e103f1bc24
  |  extra:       hg-git-rename-source=git
  |  description:
  |  add g\xc3\xa4mm\xc3\xa2 (esc)
  |
  |
  o  changeset:   1:5408f831a4d1a1d6ecccdddbe04c5a8b888a33c1
  |  parent:      0:b1884a2b1964e4881e235f33485aebc34ee61b90
  |  parent:      -1:0000000000000000000000000000000000000000
  |  manifest:    f0bd6fbafbaebe4bb59c35108428f6fce152431d
  |  user:        t\xc3\xa9st \xc3\xa8nc\xc3\xb6d\xc3\xaeng <test@example.org> (esc)
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  files+:      beta
  |  extra:       branch=default
  |  extra:       committer=test <test@example.org> 1167609611 0
  |  extra:       convert_revision=7a7e86fc1b24db03109c9fe5da28b352de59ce90
  |  extra:       hg-git-rename-source=git
  |  description:
  |  add beta
  |
  |
  o  changeset:   0:b1884a2b1964e4881e235f33485aebc34ee61b90
     parent:      -1:0000000000000000000000000000000000000000
     parent:      -1:0000000000000000000000000000000000000000
     manifest:    8b8a0e87dfd7a0706c0524afa8ba67e20544cbf0
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     files+:      alpha
     extra:       branch=default
     extra:       convert_revision=0530b75d8c203e10dc934292a6a4032c6e958a83
     extra:       hg-git-rename-source=git
     description:
     add \xc3\xa4lph\xc3\xa0 (esc)
  |  parent:      1:(9f6268bfc9eb3956c5ab8752d7b983b0ffe57115|955b24cf6f8f293741d3f39110c6fe554c292533) (re)
  

  $ hg gclear
  clearing out the git cache data
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  searching for changes
  adding objects
  added 4 commits with 4 trees and 4 blobs

  $ cd ..
  $ git --git-dir=gitrepo2 log --pretty=medium
  commit 21cee4094d142130e18dce6bd1e3a60accd6799b
  Author: t\xe9st \xe8nc\xf6d\xeeng <test@example.org> (esc)
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      add d\xe9lt\xe0 (esc)
  
  commit 4e120a5e4b8f6440e5f4aea0da3b1ddc8960186f
  Author: * <test@example.org> (glob)
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      add g*mm* (glob)
  
  commit 4840d5849378794ea269174845d7fc2ae19506a0
  Author: * <test@example.org> (glob)
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 075e54047ff498b9229fc127668d157095658b04
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add älphà
