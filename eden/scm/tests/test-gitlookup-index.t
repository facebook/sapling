#chg-compatible
#require git

Test the gitlookup.useindex=True feature for fast git -> hg commit translation.

Repos:
- gitrepo: the source git repo
- hgrepo: sync from the hgrepo
- hgclient: a dummy repo that talks to hgrepo

Prepare the git repo:

  $ . "$TESTDIR/hggit/testutil"

  $ git init --quiet gitrepo
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'

  $ git checkout --quiet -b beta
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'

  $ cd $TESTTMP

Prepare the hg repo:

  $ hg clone gitrepo hgrepo
  importing git objects into hg
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ enable gitlookup
  $ setconfig gitlookup.useindex=True gitlookup.mapfile=$TESTTMP/hgrepo/.hg/git-mapfile
  $ hg log -Gr 'all()' -T '{node} {gitnode} {bookmarks}'
  @  3bb02b6794ddc0b498cdc15f59f2e6724cabfa2f 9497a4ee62e16ee641860d7677cdb2589ea15554 beta
  |
  o  69982ec78c6dd2f24b3b62f3e2baaa79ab48ed93 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 master
  
The gitlookup interface is exposed at the wireproto layer. Use another repo to test it:

  $ newrepo hgclient
  $ hg pull -r _gitlookup_git_7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 $TESTTMP/hgrepo
  pulling from $TESTTMP/hgrepo
  importing git nodemap from flat mapfile
  building git nodemap for 2 commits
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding remote bookmark master
  $ hg pull -r _gitlookup_git_9497a4ee62e16ee641860d7677cdb2589ea15554 $TESTTMP/hgrepo
  pulling from $TESTTMP/hgrepo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding remote bookmark beta

Add new commits to test the index can be incrementally built:

  $ cd $TESTTMP/gitrepo
  $ echo segma > segma
  $ git add segma
  $ fn_git_commit -m 'add segma'

Sync git to hg:

  $ cd $TESTTMP/hgrepo
  $ hg pull
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  $ hg log -Gr 'all()' -T '{node} {gitnode} {bookmarks}'
  o  146e4a0c333d21c93eefe6bf5c01a8d51c5918ab b6d676108afa31dc39efc9c5eb57f19ecbad837b beta
  |
  @  3bb02b6794ddc0b498cdc15f59f2e6724cabfa2f 9497a4ee62e16ee641860d7677cdb2589ea15554
  |
  o  69982ec78c6dd2f24b3b62f3e2baaa79ab48ed93 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 master
  

Query the new commits:

  $ cd $TESTTMP/hgclient
  $ hg pull -r _gitlookup_git_b6d676108afa31dc39efc9c5eb57f19ecbad837b $TESTTMP/hgrepo
  pulling from $TESTTMP/hgrepo
  building git nodemap for 1 commits
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating bookmark beta

Test the debugbuildgitnodemap command. This runs the build explicitly instead of on-demand:

  $ cd $TESTTMP/hgrepo
  $ hg debugbuildgitnodemap
  0 new commits are indexed
  $ rm -rf .hg/git-nodemap .hg/git-nodemap-lastrev
  $ hg debugbuildgitnodemap
  importing git nodemap from flat mapfile
  building git nodemap for 3 commits
  3 new commits are indexed
