# reproduce issue2264, issue2516

create test repo
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > transplant =
  > graphlog =
  > EOF
  $ hg init repo
  $ cd repo
  $ template="{rev}  {desc|firstline}  [{branch}]\n"

# we need to start out with two changesets on the default branch
# in order to avoid the cute little optimization where transplant
# pulls rather than transplants
add initial changesets
  $ echo feature1 > file1
  $ hg ci -Am"feature 1"
  adding file1
  $ echo feature2 >> file2
  $ hg ci -Am"feature 2"
  adding file2

# The changes to 'bugfix' are enough to show the bug: in fact, with only
# those changes, it's a very noisy crash ("RuntimeError: nothing
# committed after transplant").  But if we modify a second file in the
# transplanted changesets, the bug is much more subtle: transplant
# silently drops the second change to 'bugfix' on the floor, and we only
# see it when we run 'hg status' after transplanting.  Subtle data loss
# bugs are worse than crashes, so reproduce the subtle case here.
commit bug fixes on bug fix branch
  $ hg branch fixes
  marked working directory as branch fixes
  (branches are permanent and global, did you want a bookmark?)
  $ echo fix1 > bugfix
  $ echo fix1 >> file1
  $ hg ci -Am"fix 1"
  adding bugfix
  $ echo fix2 > bugfix
  $ echo fix2 >> file1
  $ hg ci -Am"fix 2"
  $ hg glog --template="$template"
  @  3  fix 2  [fixes]
  |
  o  2  fix 1  [fixes]
  |
  o  1  feature 2  [default]
  |
  o  0  feature 1  [default]
  
transplant bug fixes onto release branch
  $ hg update 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg branch release
  marked working directory as branch release
  (branches are permanent and global, did you want a bookmark?)
  $ hg transplant 2 3
  applying [0-9a-f]{12} (re)
  [0-9a-f]{12} transplanted to [0-9a-f]{12} (re)
  applying [0-9a-f]{12} (re)
  [0-9a-f]{12} transplanted to [0-9a-f]{12} (re)
  $ hg glog --template="$template"
  @  5  fix 2  [release]
  |
  o  4  fix 1  [release]
  |
  | o  3  fix 2  [fixes]
  | |
  | o  2  fix 1  [fixes]
  | |
  | o  1  feature 2  [default]
  |/
  o  0  feature 1  [default]
  
  $ hg status
  $ hg status --rev 0:4
  M file1
  A bugfix
  $ hg status --rev 4:5
  M bugfix
  M file1

now test that we fixed the bug for all scripts/extensions
  $ cat > $TESTTMP/committwice.py <<__EOF__
  > from mercurial import ui, hg, match, node
  > from time import sleep
  > 
  > def replacebyte(fn, b):
  >     f = open(fn, "rb+")
  >     f.seek(0, 0)
  >     f.write(b)
  >     f.close()
  > 
  > def printfiles(repo, rev):
  >     print "revision %s files: %s" % (rev, repo[rev].files())
  > 
  > repo = hg.repository(ui.ui(), '.')
  > assert len(repo) == 6, \
  >        "initial: len(repo): %d, expected: 6" % len(repo)
  > 
  > replacebyte("bugfix", "u")
  > sleep(2)
  > try:
  >     print "PRE: len(repo): %d" % len(repo)
  >     wlock = repo.wlock()
  >     lock = repo.lock()
  >     replacebyte("file1", "x")
  >     repo.commit(text="x", user="test", date=(0, 0))
  >     replacebyte("file1", "y")
  >     repo.commit(text="y", user="test", date=(0, 0))
  >     print "POST: len(repo): %d" % len(repo)
  > finally:
  >     lock.release()
  >     wlock.release()
  > printfiles(repo, 6)
  > printfiles(repo, 7)
  > __EOF__
  $ $PYTHON $TESTTMP/committwice.py
  PRE: len(repo): 6
  POST: len(repo): 8
  revision 6 files: ['bugfix', 'file1']
  revision 7 files: ['file1']

Do a size-preserving modification outside of that process
  $ echo abcd > bugfix
  $ hg status
  M bugfix
  $ hg log --template "{rev}  {desc}  {files}\n" -r5:
  5  fix 2  bugfix file1
  6  x  bugfix file1
  7  y  file1

  $ cd ..
