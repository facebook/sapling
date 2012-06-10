Testing cloning with the EOL extension

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > eol =
  > 
  > [eol]
  > native = CRLF
  > EOF

setup repository

  $ hg init repo
  $ cd repo
  $ cat > .hgeol <<EOF
  > [patterns]
  > **.txt = native
  > EOF
  $ printf "first\r\nsecond\r\nthird\r\n" > a.txt
  $ hg commit --addremove -m 'checkin'
  adding .hgeol
  adding a.txt

Clone

  $ cd ..
  $ hg clone repo repo-2
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-2
  $ cat a.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  $ hg cat a.txt
  first
  second
  third
  $ hg remove .hgeol
  $ hg commit -m 'remove eol'
  $ hg push --quiet
  $ cd ..

Test clone of repo with .hgeol in working dir, but no .hgeol in tip

  $ hg clone repo repo-3
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-3

  $ cat a.txt
  first
  second
  third

Test clone of revision with .hgeol

  $ cd ..
  $ hg clone -r 0 repo repo-4
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-4
  $ cat .hgeol
  [patterns]
  **.txt = native

  $ cat a.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)

  $ cd ..
