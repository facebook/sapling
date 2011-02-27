Test the EOL hook

  $ cat > $HGRCPATH <<EOF
  > [diff]
  > git = True
  > EOF
  $ hg init main
  $ cat > main/.hg/hgrc <<EOF
  > [extensions]
  > eol =
  > 
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
  error: pretxnchangegroup hook failed: a.txt should not have CRLF line endings
  transaction abort!
  rollback completed
  abort: a.txt should not have CRLF line endings
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
  error: pretxnchangegroup hook failed: crlf.txt should not have LF line endings
  transaction abort!
  rollback completed
  abort: crlf.txt should not have LF line endings
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
