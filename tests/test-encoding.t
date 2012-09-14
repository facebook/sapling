# -*- coding: utf-8 -*-

# Fails for some reason, need to investigate
#   $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH
  $ echo 'hgext.graphlog =' >> $HGRCPATH

  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2007-01-01 00:00:00 +0000"; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE

  $ count=10
  $ commit()
  > {
  >     GIT_AUTHOR_DATE="2007-01-01 00:00:$count +0000"
  >     GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"
  >     git commit "$@" >/dev/null || echo "git commit error"
  >     count=`expr $count + 1`
  > }

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/

utf-8 encoded commit message
  $ echo alpha > alpha
  $ git add alpha
  $ commit -m 'add älphà'

  $ . $TESTDIR/latin-1-encoding
  Warning: commit message does not conform to UTF-8.
  You may want to amend it after fixing the message, or set the config
  variable i18n.commitencoding to the encoding your project uses.
  Warning: commit message does not conform to UTF-8.
  You may want to amend it after fixing the message, or set the config
  variable i18n.commitencoding to the encoding your project uses.

  $ cd ..
  $ mkdir gitrepo2
  $ cd gitrepo2
  $ git init --bare
  Initialized empty Git repository in $TESTTMP/gitrepo2/

  $ cd ..
  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo

  $ HGENCODING=utf-8 hg log --graph --debug | grep -v ': *master' | grep -v phase:
  @  changeset:   3:8549ee7fe0801b2dafc06047ca6f66d36da709f5
  |  tag:         default/master
  |  tag:         tip
  |  parent:      2:0422fbb4ec39fb69e87b94a3874ac890333de11a
  |  parent:      -1:0000000000000000000000000000000000000000
  |  manifest:    3:ea49f93388380ead5601c8fcbfa187516e7c2ed8
  |  user:        tést èncödîng <test@example.org>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  files+:      delta
  |  extra:       author=$ \x90\x01\x01\xe9\x91\x03\x03\x01\xe8\x91\x08\x02\x01\xf6\x91\x0c\x01\x01\xee\x91\x0f\x15
  |  extra:       branch=default
  |  extra:       committer=test <test@example.org> 1167609613 0
  |  extra:       encoding=latin-1
  |  extra:       message=\x0c\n\x90\x05\x01\xe9\x91\x07\x02\x01\xe0\x91\x0b\x01
  |  description:
  |  add déltà
  |
  |
  o  changeset:   2:0422fbb4ec39fb69e87b94a3874ac890333de11a
  |  parent:      1:9f6268bfc9eb3956c5ab8752d7b983b0ffe57115
  |  parent:      -1:0000000000000000000000000000000000000000
  |  manifest:    2:f580e7da3673c137370da2b931a1dee83590d7b4
  |  user:        tést èncödîng <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  files+:      gamma
  |  extra:       author=$ \x90\x01\x01\xe9\x91\x03\x03\x01\xe8\x91\x08\x02\x01\xf6\x91\x0c\x01\x01\xee\x91\x0f\x15
  |  extra:       branch=default
  |  extra:       committer=test <test@example.org> 1167609612 0
  |  extra:       message=\x0c\n\x90\x05\x01\xe4\x91\x07\x02\x01\xe2\x91\x0b\x01
  |  description:
  |  add gämmâ
  |
  |
  o  changeset:   1:9f6268bfc9eb3956c5ab8752d7b983b0ffe57115
  |  parent:      0:bb7d36568d6188ce0de2392246c43f6f213df954
  |  parent:      -1:0000000000000000000000000000000000000000
  |  manifest:    1:f0bd6fbafbaebe4bb59c35108428f6fce152431d
  |  user:        tést èncödîng <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  files+:      beta
  |  extra:       author=$ \x90\x01\x01\xe9\x91\x03\x03\x01\xe8\x91\x08\x02\x01\xf6\x91\x0c\x01\x01\xee\x91\x0f\x15
  |  extra:       branch=default
  |  extra:       committer=test <test@example.org> 1167609611 0
  |  description:
  |  add beta
  |
  |
  o  changeset:   0:bb7d36568d6188ce0de2392246c43f6f213df954
     parent:      -1:0000000000000000000000000000000000000000
     parent:      -1:0000000000000000000000000000000000000000
     manifest:    0:8b8a0e87dfd7a0706c0524afa8ba67e20544cbf0
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     files+:      alpha
     extra:       branch=default
     description:
     add älphà
  
  

  $ hg gclear
  clearing out the git cache data
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  exporting hg objects to git
  creating and sending data

  $ cd ../gitrepo2
  $ git log --pretty=medium
  commit da0edb01d4f3d1abf08b1be298379b0b2960e680
  Author: t\xe9st \xe8nc\xf6d\xeeng <test@example.org> (esc)
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      add d\xe9lt\xe0 (esc)
  
  commit 2372b6c8f1b91f2db8ae5eb0f9e0427c318b449c
  Author: t\xe9st \xe8nc\xf6d\xeeng <test@example.org> (esc)
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      add g\xe4mm\xe2 (esc)
  
  commit 9ef7f6dcffe643b89ba63f3323621b9a923e4802
  Author: t\xe9st \xe8nc\xf6d\xeeng <test@example.org> (esc)
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 0530b75d8c203e10dc934292a6a4032c6e958a83
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add älphà

  $ cd ..
