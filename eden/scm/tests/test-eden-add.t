
#require eden

setup backing repo
  $ newclientrepo

  $ touch rootfile.txt
  $ mkdir dir1
  $ touch dir1/a.txt
  $ echo "original contents" >> dir1/a.txt
  $ hg add rootfile.txt dir1/a.txt
  $ hg commit -m "Initial commit."

test basic hg add operations

  $ touch dir1/b.txt
  $ mkdir dir2
  $ touch dir2/c.txt

  $ hg status
  ? dir1/b.txt
  ? dir2/c.txt

  $ hg debugdirstate --json
  {}

  $ hg add dir2
  adding dir2/c.txt

  $ hg status
  A dir2/c.txt
  ? dir1/b.txt

  $ hg debugdirstate --json
  {"dir2/c.txt": {"merge_state": -1, "merge_state_string": "MERGE_BOTH", "mode": 0, "status": "a"}}

  $ hg rm --force dir1/a.txt
  $ echo "original contents" > dir1/a.txt
  $ touch dir1/a.txt

  $ hg status
  A dir2/c.txt
  R dir1/a.txt
  ? dir1/b.txt

  $ hg add .
  adding dir1/a.txt
  adding dir1/b.txt

  $ hg status
  A dir1/b.txt
  A dir2/c.txt

  $ hg rm dir1/a.txt
  $ echo "different contents" > dir1/a.txt
  $ hg add dir1
  adding dir1/a.txt

  $ hg status
  M dir1/a.txt
  A dir1/b.txt
  A dir2/c.txt

  $ hg rm --force dir1/a.txt
  $ hg add dir1

  $ hg status
  A dir1/b.txt
  A dir2/c.txt
  R dir1/a.txt

  $ hg add dir3
  dir3: $ENOENT$
  [1]
