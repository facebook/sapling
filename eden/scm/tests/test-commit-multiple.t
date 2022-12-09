#chg-compatible
#debugruntest-compatible

# reproduce issue2264, issue2516

  $ setconfig devel.segmented-changelog-rev-compat=true
create test repo
  $ hg init repo
  $ cd repo
  $ template="{desc|firstline}  [{branch}]\n"

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
  $ echo fix1 > bugfix
  $ echo fix1 >> file1
  $ hg ci -Am"fix 1"
  adding bugfix
  $ echo fix2 > bugfix
  $ echo fix2 >> file1
  $ hg ci -Am"fix 2"
  $ hg log -G --template="$template"
  @  fix 2  [default]
  │
  o  fix 1  [default]
  │
  o  feature 2  [default]
  │
  o  feature 1  [default]
  
transplant bug fixes onto release branch
  $ hg goto 23224ffa3bb0b0d891aeadc0bd225327c05426ca
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg graft 1f6b59d373ef839c739a0f64390ef31d3bf14c21
  grafting 1f6b59d373ef "fix 1"
  $ hg graft a53b0210149009b7d475a30645c468cf3678a442
  grafting a53b02101490 "fix 2"
  $ hg log -G --template="$template"
  @  fix 2  [default]
  │
  o  fix 1  [default]
  │
  │ o  fix 2  [default]
  │ │
  │ o  fix 1  [default]
  │ │
  │ o  feature 2  [default]
  ├─╯
  o  feature 1  [default]
  
  $ hg status
  $ hg status --rev 23224ffa3bb0b0d891aeadc0bd225327c05426ca:17595d510ef52376aeb0436ad9a0593d626bbbec
  M file1
  A bugfix
  $ hg status --rev 17595d510ef52376aeb0436ad9a0593d626bbbec:'max(desc(fix))'
  M bugfix
  M file1

now test that we fixed the bug for all scripts/extensions
  $ cat > $TESTTMP/committwice.py <<__EOF__
  > from edenscm import ui, hg, match, node
  > from time import sleep
  > 
  > def replacebyte(fn, b):
  >     f = open(fn, "rb+")
  >     f.seek(0, 0)
  >     f.write(b)
  >     f.close()
  > 
  > def printfiles(repo, rev):
  >     print("revision %s files: %s" % (rev, repo[rev].files()))
  > 
  > repo = hg.repository(ui.ui.load(), '.')
  > assert len(repo) == 6, "initial: len(repo): %d, expected: 6" % len(repo)
  > 
  > replacebyte("bugfix", b"u")
  > sleep(2)
  > try:
  >     print("PRE: len(repo): %d" % len(repo))
  >     wlock = repo.wlock()
  >     lock = repo.lock()
  >     replacebyte("file1", b"x")
  >     repo.commit(text="x", user="test", date=(0, 0))
  >     replacebyte("file1", b"y")
  >     repo.commit(text="y", user="test", date=(0, 0))
  >     print("POST: len(repo): %d" % len(repo))
  > finally:
  >     lock.release()
  >     wlock.release()
  > printfiles(repo, 6)
  > printfiles(repo, 7)
  > __EOF__
  $ hg debugpython -- $TESTTMP/committwice.py
  PRE: len(repo): 6
  POST: len(repo): 8
  revision 6 files: ('bugfix', 'file1')
  revision 7 files: ('file1',)

Do a size-preserving modification outside of that process
  $ echo abcd > bugfix
  $ hg status
  M bugfix
  $ hg log --template "{desc}  {files}\n" -r'max(desc(fix))':
  fix 2  bugfix file1
  x  bugfix file1
  y  file1

  $ cd ..
