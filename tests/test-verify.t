prepare repo

  $ hg init a
  $ cd a
  $ echo "some text" > FOO.txt
  $ echo "another text" > bar.txt
  $ echo "more text" > QUICK.txt
  $ hg add
  adding FOO.txt
  adding QUICK.txt
  adding bar.txt
  $ hg ci -mtest1

verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 1 changesets, 3 total revisions

verify with journal

  $ touch .hg/store/journal
  $ hg verify
  abandoned transaction found - run hg recover
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 1 changesets, 3 total revisions
  $ rm .hg/store/journal

introduce some bugs in repo

  $ cd .hg/store/data
  $ mv _f_o_o.txt.i X_f_o_o.txt.i
  $ mv bar.txt.i xbar.txt.i
  $ rm _q_u_i_c_k.txt.i

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   data/FOO.txt.i@0: missing revlog!
   0: empty or missing FOO.txt
   FOO.txt@0: f62022d3d590 in manifests not found
   data/QUICK.txt.i@0: missing revlog!
   0: empty or missing QUICK.txt
   QUICK.txt@0: 88b857db8eba in manifests not found
   data/bar.txt.i@0: missing revlog!
   0: empty or missing bar.txt
   bar.txt@0: 256559129457 in manifests not found
  3 files, 1 changesets, 0 total revisions
  9 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]

  $ cd ..

test revlog corruption

  $ hg init b
  $ cd b

  $ touch a
  $ hg add a
  $ hg ci -m a

  $ echo 'corrupted' > b
  $ dd if=.hg/store/data/a.i of=start bs=1 count=20 2>/dev/null
  $ cat start b > .hg/store/data/a.i

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   a@0: broken revlog! (index data/a.i is corrupted)
  warning: orphan revlog 'data/a.i'
  1 files, 1 changesets, 0 total revisions
  1 warnings encountered!
  1 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]

  $ cd ..

test revlog format 0

  $ "$TESTDIR/revlog-formatv0.py"
  $ cd formatv0
  $ hg verify
  repository uses revlog format 0
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
