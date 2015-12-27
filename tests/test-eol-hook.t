Test the EOL hook

  $ hg init main
  $ cat > main/.hg/hgrc <<EOF
  > [hooks]
  > pretxnchangegroup = python:hgext.eol.hook
  > EOF
  $ hg clone main fork
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd fork

Create repo
  $ cat > .hgeol <<EOF
  > [patterns]
  > mixed.txt = BIN
  > crlf.txt = CRLF
  > **.txt = native
  > EOF
  $ hg add .hgeol
  $ hg commit -m 'Commit .hgeol'

  $ printf "first\nsecond\nthird\n" > a.txt
  $ hg add a.txt
  $ hg commit -m 'LF a.txt'
  $ hg push ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files

  $ printf "first\r\nsecond\r\nthird\n" > a.txt
  $ hg commit -m 'CRLF a.txt'
  $ hg push ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  error: pretxnchangegroup hook failed: end-of-line check failed:
    a.txt in a8ee6548cd86 should not have CRLF line endings
  transaction abort!
  rollback completed
  abort: end-of-line check failed:
    a.txt in a8ee6548cd86 should not have CRLF line endings
  [255]

  $ printf "first\nsecond\nthird\n" > a.txt
  $ hg commit -m 'LF a.txt (fixed)'
  $ hg push ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files

  $ printf "first\nsecond\nthird\n" > crlf.txt
  $ hg add crlf.txt
  $ hg commit -m 'LF crlf.txt'
  $ hg push ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  error: pretxnchangegroup hook failed: end-of-line check failed:
    crlf.txt in 004ba2132725 should not have LF line endings
  transaction abort!
  rollback completed
  abort: end-of-line check failed:
    crlf.txt in 004ba2132725 should not have LF line endings
  [255]

  $ printf "first\r\nsecond\r\nthird\r\n" > crlf.txt
  $ hg commit -m 'CRLF crlf.txt (fixed)'
  $ hg push ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files

  $ printf "first\r\nsecond" > b.txt
  $ hg add b.txt
  $ hg commit -m 'CRLF b.txt'
  $ hg push ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  error: pretxnchangegroup hook failed: end-of-line check failed:
    b.txt in fbcf9b1025f5 should not have CRLF line endings
  transaction abort!
  rollback completed
  abort: end-of-line check failed:
    b.txt in fbcf9b1025f5 should not have CRLF line endings
  [255]

  $ hg up -r -2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ printf "some\nother\nfile" > c.txt
  $ hg add c.txt
  $ hg commit -m "LF c.txt, b.txt doesn't exist here"
  created new head
  $ hg push -f ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  error: pretxnchangegroup hook failed: end-of-line check failed:
    b.txt in fbcf9b1025f5 should not have CRLF line endings
  transaction abort!
  rollback completed
  abort: end-of-line check failed:
    b.txt in fbcf9b1025f5 should not have CRLF line endings
  [255]

Test checkheadshook alias

  $ cat > ../main/.hg/hgrc <<EOF
  > [hooks]
  > pretxnchangegroup = python:hgext.eol.checkheadshook
  > EOF
  $ hg push -f ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  error: pretxnchangegroup hook failed: end-of-line check failed:
    b.txt in fbcf9b1025f5 should not have CRLF line endings
  transaction abort!
  rollback completed
  abort: end-of-line check failed:
    b.txt in fbcf9b1025f5 should not have CRLF line endings
  [255]

We can fix the head and push again

  $ hg up 6
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ printf "first\nsecond" > b.txt
  $ hg ci -m "remove CRLF from b.txt"
  $ hg push -f ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 2 files (+1 heads)
  $ hg -R ../main rollback
  repository tip rolled back to revision 5 (undo push)

Test it still fails with checkallhook

  $ cat > ../main/.hg/hgrc <<EOF
  > [hooks]
  > pretxnchangegroup = python:hgext.eol.checkallhook
  > EOF
  $ hg push -f ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 2 files (+1 heads)
  error: pretxnchangegroup hook failed: end-of-line check failed:
    b.txt in fbcf9b1025f5 should not have CRLF line endings
  transaction abort!
  rollback completed
  abort: end-of-line check failed:
    b.txt in fbcf9b1025f5 should not have CRLF line endings
  [255]

But we can push the clean head

  $ hg push -r7 -f ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Test multiple files/revisions output

  $ printf "another\r\nbad\r\none" > d.txt
  $ hg add d.txt
  $ hg ci -m "add d.txt"
  $ hg push -f ../main
  pushing to ../main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 2 files (+1 heads)
  error: pretxnchangegroup hook failed: end-of-line check failed:
    b.txt in fbcf9b1025f5 should not have CRLF line endings
    d.txt in a7040e68714f should not have CRLF line endings
  transaction abort!
  rollback completed
  abort: end-of-line check failed:
    b.txt in fbcf9b1025f5 should not have CRLF line endings
    d.txt in a7040e68714f should not have CRLF line endings
  [255]

  $ cd ..
