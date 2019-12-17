Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'

  $ cd ..

  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo log --graph
  @  changeset:   1:3bb02b6794dd
  |  bookmark:    master
  |  tag:         default/master
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  o  changeset:   0:69982ec78c6d
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

we should have some bookmarks
  $ hg -R hgrepo book
   * master                    1:3bb02b6794dd
  $ hg -R hgrepo gverify
  verifying rev 3bb02b6794dd against git commit 9497a4ee62e16ee641860d7677cdb2589ea15554

test for ssh vulnerability

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh = ssh -o ConnectTimeout=1
  > EOF

  $ hg clone 'git+ssh://-oProxyCommand=rm${IFS}nonexistent/path' 2>&1 >/dev/null
  abort: potentially unsafe hostname: '-oProxyCommand=rm${IFS}nonexistent'
  [255]
  $ hg clone 'git+ssh://%2DoProxyCommand=rm${IFS}nonexistent/path' 2>&1 >/dev/null
  abort: potentially unsafe hostname: '-oProxyCommand=rm${IFS}nonexistent'
  [255]
  $ hg clone 'git+ssh://fakehost|rm${IFS}nonexistent/path'
  ssh: .* fakehost%7[Cc]rm%24%7[Bb][Ii][Ff][Ss]%7[Dd]nonexistent.* (re)
  destination directory: path
  abort: git remote error: The remote server unexpectedly closed the connection.
  [255]
  $ hg clone 'git+ssh://fakehost%7Crm${IFS}nonexistent/path'
  ssh: .* fakehost%7[cC]rm%24%7[Bb][Ii][Ff][Ss]%7[Dd]nonexistent.* (re)
  destination directory: path
  abort: git remote error: The remote server unexpectedly closed the connection.
  [255]
