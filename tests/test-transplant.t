#require killdaemons

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > transplant=
  > EOF

  $ hg init t
  $ cd t
  $ hg transplant
  abort: no source URL, branch revision, or revision list provided
  [255]
  $ hg transplant --continue --all
  abort: --continue is incompatible with --branch, --all and --merge
  [255]
  $ hg transplant --all tip
  abort: --all requires a branch revision
  [255]
  $ hg transplant --all --branch default tip
  abort: --all is incompatible with a revision list
  [255]
  $ echo r1 > r1
  $ hg ci -Amr1 -d'0 0'
  adding r1
  $ hg co -q null
  $ hg transplant tip
  abort: no revision checked out
  [255]
  $ hg up -q
  $ echo r2 > r2
  $ hg ci -Amr2 -d'1 0'
  adding r2
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo b1 > b1
  $ hg ci -Amb1 -d '0 0'
  adding b1
  created new head
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg transplant 1
  abort: outstanding uncommitted merges
  [255]
  $ hg up -qC tip
  $ echo b0 > b1
  $ hg transplant 1
  abort: outstanding local changes
  [255]
  $ hg up -qC tip
  $ echo b2 > b2
  $ hg ci -Amb2 -d '1 0'
  adding b2
  $ echo b3 > b3
  $ hg ci -Amb3 -d '2 0'
  adding b3

  $ hg log --template '{rev} {parents} {desc}\n'
  4  b3
  3  b2
  2 0:17ab29e464c6  b1
  1  r2
  0  r1

  $ hg clone . ../rebase
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg init ../emptydest
  $ cd ../emptydest
  $ hg transplant --source=../t > /dev/null
  $ cd ../rebase

  $ hg up -C 1
  1 files updated, 0 files merged, 3 files removed, 0 files unresolved

rebase b onto r1
(this also tests that editor is not invoked if '--edit' is not specified)

  $ HGEDITOR=cat hg transplant -a -b tip
  applying 37a1297eb21b
  37a1297eb21b transplanted to e234d668f844
  applying 722f4667af76
  722f4667af76 transplanted to 539f377d78df
  applying a53251cdf717
  a53251cdf717 transplanted to ffd6818a3975
  $ hg log --template '{rev} {parents} {desc}\n'
  7  b3
  6  b2
  5 1:d11e3596cc1a  b1
  4  b3
  3  b2
  2 0:17ab29e464c6  b1
  1  r2
  0  r1

test transplanted revset

  $ hg log -r 'transplanted()' --template '{rev} {parents} {desc}\n'
  5 1:d11e3596cc1a  b1
  6  b2
  7  b3
  $ hg log -r 'transplanted(head())' --template '{rev} {parents} {desc}\n'
  7  b3
  $ hg help revsets | grep transplanted
      "transplanted([set])"
        Transplanted changesets in set, or all transplanted changesets.

test transplanted keyword

  $ hg log --template '{rev} {transplanted}\n'
  7 a53251cdf717679d1907b289f991534be05c997a
  6 722f4667af767100cb15b6a79324bf8abbfe1ef4
  5 37a1297eb21b3ef5c5d2ffac22121a0988ed9f21
  4 
  3 
  2 
  1 
  0 

test destination() revset predicate with a transplant of a transplant; new
clone so subsequent rollback isn't affected
(this also tests that editor is invoked if '--edit' is specified)

  $ hg clone -q . ../destination
  $ cd ../destination
  $ hg up -Cq 0
  $ hg branch -q b4
  $ hg ci -qm "b4"
  $ hg status --rev "7^1" --rev 7
  A b3
  $ cat > $TESTTMP/checkeditform.sh <<EOF
  > env | grep HGEDITFORM
  > true
  > EOF
  $ cat > $TESTTMP/checkeditform-n-cat.sh <<EOF
  > env | grep HGEDITFORM
  > cat \$*
  > EOF
  $ HGEDITOR="sh $TESTTMP/checkeditform-n-cat.sh" hg transplant --edit 7
  applying ffd6818a3975
  HGEDITFORM=transplant.normal
  b3
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'b4'
  HG: added b3
  ffd6818a3975 transplanted to 502236fa76bb


  $ hg log -r 'destination()'
  changeset:   5:e234d668f844
  parent:      1:d11e3596cc1a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b1
  
  changeset:   6:539f377d78df
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2
  
  changeset:   7:ffd6818a3975
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b3
  
  changeset:   9:502236fa76bb
  branch:      b4
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b3
  
  $ hg log -r 'destination(a53251cdf717)'
  changeset:   7:ffd6818a3975
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b3
  
  changeset:   9:502236fa76bb
  branch:      b4
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b3
  

test subset parameter in reverse order
  $ hg log -r 'reverse(all()) and destination(a53251cdf717)'
  changeset:   9:502236fa76bb
  branch:      b4
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b3
  
  changeset:   7:ffd6818a3975
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b3
  

back to the original dir
  $ cd ../rebase

rollback the transplant
  $ hg rollback
  repository tip rolled back to revision 4 (undo transplant)
  working directory now based on revision 1
  $ hg tip -q
  4:a53251cdf717
  $ hg parents -q
  1:d11e3596cc1a
  $ hg status
  ? b1
  ? b2
  ? b3

  $ hg clone ../t ../prune
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../prune

  $ hg up -C 1
  1 files updated, 0 files merged, 3 files removed, 0 files unresolved

rebase b onto r1, skipping b2

  $ hg transplant -a -b tip -p 3
  applying 37a1297eb21b
  37a1297eb21b transplanted to e234d668f844
  applying a53251cdf717
  a53251cdf717 transplanted to 7275fda4d04f
  $ hg log --template '{rev} {parents} {desc}\n'
  6  b3
  5 1:d11e3596cc1a  b1
  4  b3
  3  b2
  2 0:17ab29e464c6  b1
  1  r2
  0  r1

test same-parent transplant with --log

  $ hg clone -r 1 ../t ../sameparent
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../sameparent
  $ hg transplant --log -s ../prune 5
  searching for changes
  applying e234d668f844
  e234d668f844 transplanted to e07aea8ecf9c
  $ hg log --template '{rev} {parents} {desc}\n'
  2  b1
  (transplanted from e234d668f844e1b1a765f01db83a32c0c7bfa170)
  1  r2
  0  r1
remote transplant, and also test that transplant doesn't break with
format-breaking diffopts

  $ hg clone -r 1 ../t ../remote
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../remote
  $ hg --config diff.noprefix=True transplant --log -s ../t 2 4
  searching for changes
  applying 37a1297eb21b
  37a1297eb21b transplanted to c19cf0ccb069
  applying a53251cdf717
  a53251cdf717 transplanted to f7fe5bf98525
  $ hg log --template '{rev} {parents} {desc}\n'
  3  b3
  (transplanted from a53251cdf717679d1907b289f991534be05c997a)
  2  b1
  (transplanted from 37a1297eb21b3ef5c5d2ffac22121a0988ed9f21)
  1  r2
  0  r1

skip previous transplants

  $ hg transplant -s ../t -a -b 4
  searching for changes
  applying 722f4667af76
  722f4667af76 transplanted to 47156cd86c0b
  $ hg log --template '{rev} {parents} {desc}\n'
  4  b2
  3  b3
  (transplanted from a53251cdf717679d1907b289f991534be05c997a)
  2  b1
  (transplanted from 37a1297eb21b3ef5c5d2ffac22121a0988ed9f21)
  1  r2
  0  r1

skip local changes transplanted to the source

  $ echo b4 > b4
  $ hg ci -Amb4 -d '3 0'
  adding b4
  $ hg clone ../t ../pullback
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../pullback
  $ hg transplant -s ../remote -a -b tip
  searching for changes
  applying 4333daefcb15
  4333daefcb15 transplanted to 5f42c04e07cc


remote transplant with pull

  $ hg serve -R ../t -p $HGPORT -d --pid-file=../t.pid
  $ cat ../t.pid >> $DAEMON_PIDS

  $ hg clone -r 0 ../t ../rp
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../rp
  $ hg transplant -s http://localhost:$HGPORT/ 37a1297eb21b a53251cdf717
  searching for changes
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  applying a53251cdf717
  a53251cdf717 transplanted to 8d9279348abb
  $ hg log --template '{rev} {parents} {desc}\n'
  2  b3
  1  b1
  0  r1

remote transplant without pull
(It was using "2" and "4" (as the previous transplant used to) which referenced
revision different from one run to another)

  $ hg pull -q http://localhost:$HGPORT/
  $ hg transplant -s http://localhost:$HGPORT/ 8d9279348abb 722f4667af76
  skipping already applied revision 2:8d9279348abb
  applying 722f4667af76
  722f4667af76 transplanted to 76e321915884

transplant --continue

  $ hg init ../tc
  $ cd ../tc
  $ cat <<EOF > foo
  > foo
  > bar
  > baz
  > EOF
  $ echo toremove > toremove
  $ echo baz > baz
  $ hg ci -Amfoo
  adding baz
  adding foo
  adding toremove
  $ cat <<EOF > foo
  > foo2
  > bar2
  > baz2
  > EOF
  $ rm toremove
  $ echo added > added
  $ hg ci -Amfoo2
  adding added
  removing toremove
  $ echo bar > bar
  $ cat > baz <<EOF
  > before baz
  > baz
  > after baz
  > EOF
  $ hg ci -Ambar
  adding bar
  $ echo bar2 >> bar
  $ hg ci -mbar2
  $ hg up 0
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo foobar > foo
  $ hg ci -mfoobar
  created new head
  $ hg transplant 1:3
  applying 46ae92138f3c
  patching file foo
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file foo.rej
  patch failed to apply
  abort: fix up the working directory and run hg transplant --continue
  [255]

transplant -c shouldn't use an old changeset

  $ hg up -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"
  $ rm added
  $ hg transplant --continue
  abort: no transplant to continue
  [255]
  $ hg transplant 1
  applying 46ae92138f3c
  patching file foo
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file foo.rej
  patch failed to apply
  abort: fix up the working directory and run hg transplant --continue
  [255]
  $ cp .hg/transplant/journal .hg/transplant/journal.orig
  $ cat .hg/transplant/journal
  # User test
  # Date 0 0
  # Node ID 46ae92138f3ce0249f6789650403286ead052b6d
  # Parent e8643552fde58f57515e19c4b373a57c96e62af3
  foo2
  $ grep -v 'Date' .hg/transplant/journal.orig > .hg/transplant/journal
  $ HGEDITOR="sh $TESTTMP/checkeditform.sh" hg transplant --continue -e
  abort: filter corrupted changeset (no user or date)
  [255]
  $ cp .hg/transplant/journal.orig .hg/transplant/journal
  $ HGEDITOR="sh $TESTTMP/checkeditform.sh" hg transplant --continue -e
  HGEDITFORM=transplant.normal
  46ae92138f3c transplanted as 9159dada197d
  $ hg transplant 1:3
  skipping already applied revision 1:46ae92138f3c
  applying 9d6d6b5a8275
  9d6d6b5a8275 transplanted to 2d17a10c922f
  applying 1dab759070cf
  1dab759070cf transplanted to e06a69927eb0
  $ hg locate
  added
  bar
  baz
  foo

test multiple revisions and --continue

  $ hg up -qC 0
  $ echo bazbaz > baz
  $ hg ci -Am anotherbaz baz
  created new head
  $ hg transplant 1:3
  applying 46ae92138f3c
  46ae92138f3c transplanted to 1024233ea0ba
  applying 9d6d6b5a8275
  patching file baz
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file baz.rej
  patch failed to apply
  abort: fix up the working directory and run hg transplant --continue
  [255]
  $ hg transplant 1:3
  abort: transplant in progress
  (use 'hg transplant --continue' or 'hg update' to abort)
  [255]
  $ echo fixed > baz
  $ hg transplant --continue
  9d6d6b5a8275 transplanted as d80c49962290
  applying 1dab759070cf
  1dab759070cf transplanted to aa0ffe6bd5ae

  $ cd ..

Issue1111: Test transplant --merge

  $ hg init t1111
  $ cd t1111
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo b >> a
  $ hg ci -m appendb
  $ echo c >> a
  $ hg ci -m appendc
  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo d >> a
  $ hg ci -m appendd
  created new head

transplant

  $ HGEDITOR="sh $TESTTMP/checkeditform.sh" hg transplant -m 1 -e
  applying 42dc4432fd35
  HGEDITFORM=transplant.merge
  1:42dc4432fd35 merged at a9f4acbac129
  $ hg update -q -C 2
  $ cat > a <<EOF
  > x
  > y
  > z
  > EOF
  $ hg commit -m replace
  $ hg update -q -C 4
  $ hg transplant -m 5
  applying 600a3cdcb41d
  patching file a
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file a.rej
  patch failed to apply
  abort: fix up the working directory and run hg transplant --continue
  [255]
  $ HGEDITOR="sh $TESTTMP/checkeditform.sh" hg transplant --continue -e
  HGEDITFORM=transplant.merge
  600a3cdcb41d transplanted as a3f88be652e0

  $ cd ..

test transplant into empty repository

  $ hg init empty
  $ cd empty
  $ hg transplant -s ../t -b tip -a
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 4 files

test "--merge" causing pull from source repository on local host

  $ hg --config extensions.mq= -q strip 2
  $ hg transplant -s ../t --merge tip
  searching for changes
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  applying a53251cdf717
  4:a53251cdf717 merged at 4831f4dc831a

test interactive transplant

  $ hg --config extensions.strip= -q strip 0
  $ hg -R ../t log -G --template "{rev}:{node|short}"
  @  4:a53251cdf717
  |
  o  3:722f4667af76
  |
  o  2:37a1297eb21b
  |
  | o  1:d11e3596cc1a
  |/
  o  0:17ab29e464c6
  
  $ hg transplant -q --config ui.interactive=true -s ../t <<EOF
  > ?
  > x
  > q
  > EOF
  0:17ab29e464c6
  apply changeset? [ynmpcq?]: ?
  y: yes, transplant this changeset
  n: no, skip this changeset
  m: merge at this changeset
  p: show patch
  c: commit selected changesets
  q: quit and cancel transplant
  ?: ? (show this help)
  apply changeset? [ynmpcq?]: x
  unrecognized response
  apply changeset? [ynmpcq?]: q
  $ hg transplant -q --config ui.interactive=true -s ../t <<EOF
  > p
  > y
  > n
  > n
  > m
  > c
  > EOF
  0:17ab29e464c6
  apply changeset? [ynmpcq?]: p
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/r1	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +r1
  apply changeset? [ynmpcq?]: y
  1:d11e3596cc1a
  apply changeset? [ynmpcq?]: n
  2:37a1297eb21b
  apply changeset? [ynmpcq?]: n
  3:722f4667af76
  apply changeset? [ynmpcq?]: m
  4:a53251cdf717
  apply changeset? [ynmpcq?]: c
  $ hg log -G --template "{node|short}"
  @    88be5dde5260
  |\
  | o  722f4667af76
  | |
  | o  37a1297eb21b
  |/
  o  17ab29e464c6
  
  $ hg transplant -q --config ui.interactive=true -s ../t <<EOF
  > x
  > ?
  > y
  > q
  > EOF
  1:d11e3596cc1a
  apply changeset? [ynmpcq?]: x
  unrecognized response
  apply changeset? [ynmpcq?]: ?
  y: yes, transplant this changeset
  n: no, skip this changeset
  m: merge at this changeset
  p: show patch
  c: commit selected changesets
  q: quit and cancel transplant
  ?: ? (show this help)
  apply changeset? [ynmpcq?]: y
  4:a53251cdf717
  apply changeset? [ynmpcq?]: q
  $ hg heads --template "{node|short}\n"
  88be5dde5260

  $ cd ..


#if unix-permissions system-sh

test filter

  $ hg init filter
  $ cd filter
  $ cat <<'EOF' >test-filter
  > #!/bin/sh
  > sed 's/r1/r2/' $1 > $1.new
  > mv $1.new $1
  > EOF
  $ chmod +x test-filter
  $ hg transplant -s ../t -b tip -a --filter ./test-filter
  filtering * (glob)
  applying 17ab29e464c6
  17ab29e464c6 transplanted to e9ffc54ea104
  filtering * (glob)
  applying 37a1297eb21b
  37a1297eb21b transplanted to 348b36d0b6a5
  filtering * (glob)
  applying 722f4667af76
  722f4667af76 transplanted to 0aa6979afb95
  filtering * (glob)
  applying a53251cdf717
  a53251cdf717 transplanted to 14f8512272b5
  $ hg log --template '{rev} {parents} {desc}\n'
  3  b3
  2  b2
  1  b1
  0  r2
  $ cd ..


test filter with failed patch

  $ cd filter
  $ hg up 0
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo foo > b1
  $ hg ci -Am foo
  adding b1
  adding test-filter
  created new head
  $ hg transplant 1 --filter ./test-filter
  filtering * (glob)
  applying 348b36d0b6a5
  file b1 already exists
  1 out of 1 hunks FAILED -- saving rejects to file b1.rej
  patch failed to apply
  abort: fix up the working directory and run hg transplant --continue
  [255]
  $ cd ..

test environment passed to filter

  $ hg init filter-environment
  $ cd filter-environment
  $ cat <<'EOF' >test-filter-environment
  > #!/bin/sh
  > echo "Transplant by $HGUSER" >> $1
  > echo "Transplant from rev $HGREVISION" >> $1
  > EOF
  $ chmod +x test-filter-environment
  $ hg transplant -s ../t --filter ./test-filter-environment 0
  filtering * (glob)
  applying 17ab29e464c6
  17ab29e464c6 transplanted to 5190e68026a0

  $ hg log --template '{rev} {parents} {desc}\n'
  0  r1
  Transplant by test
  Transplant from rev 17ab29e464c6ca53e329470efe2a9918ac617a6f
  $ cd ..

test transplant with filter handles invalid changelog

  $ hg init filter-invalid-log
  $ cd filter-invalid-log
  $ cat <<'EOF' >test-filter-invalid-log
  > #!/bin/sh
  > echo "" > $1
  > EOF
  $ chmod +x test-filter-invalid-log
  $ hg transplant -s ../t --filter ./test-filter-invalid-log 0
  filtering * (glob)
  abort: filter corrupted changeset (no user or date)
  [255]
  $ cd ..

#endif


test with a win32ext like setup (differing EOLs)

  $ hg init twin1
  $ cd twin1
  $ echo a > a
  $ echo b > b
  $ echo b >> b
  $ hg ci -Am t
  adding a
  adding b
  $ echo a > b
  $ echo b >> b
  $ hg ci -m changeb
  $ cd ..

  $ hg init twin2
  $ cd twin2
  $ echo '[patch]' >> .hg/hgrc
  $ echo 'eol = crlf' >> .hg/hgrc
  $ $PYTHON -c "file('b', 'wb').write('b\r\nb\r\n')"
  $ hg ci -Am addb
  adding b
  $ hg transplant -s ../twin1 tip
  searching for changes
  warning: repository is unrelated
  applying 2e849d776c17
  2e849d776c17 transplanted to 8e65bebc063e
  $ cat b
  a\r (esc)
  b\r (esc)
  $ cd ..

test transplant with merge changeset is skipped

  $ hg init merge1a
  $ cd merge1a
  $ echo a > a
  $ hg ci -Am a
  adding a
  $ hg branch b
  marked working directory as branch b
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m branchb
  $ echo b > b
  $ hg ci -Am b
  adding b
  $ hg update default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m mergeb
  $ cd ..

  $ hg init merge1b
  $ cd merge1b
  $ hg transplant -s ../merge1a tip
  $ cd ..

test transplant with merge changeset accepts --parent

  $ hg init merge2a
  $ cd merge2a
  $ echo a > a
  $ hg ci -Am a
  adding a
  $ hg branch b
  marked working directory as branch b
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m branchb
  $ echo b > b
  $ hg ci -Am b
  adding b
  $ hg update default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m mergeb
  $ cd ..

  $ hg init merge2b
  $ cd merge2b
  $ hg transplant -s ../merge2a --parent tip tip
  abort: be9f9b39483f is not a parent of be9f9b39483f
  [255]
  $ hg transplant -s ../merge2a --parent 0 tip
  applying be9f9b39483f
  be9f9b39483f transplanted to 9959e51f94d1
  $ cd ..

test transplanting a patch turning into a no-op

  $ hg init binarysource
  $ cd binarysource
  $ echo a > a
  $ hg ci -Am adda a
  >>> file('b', 'wb').write('\0b1')
  $ hg ci -Am addb b
  >>> file('b', 'wb').write('\0b2')
  $ hg ci -m changeb b
  $ cd ..

  $ hg clone -r0 binarysource binarydest
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd binarydest
  $ cp ../binarysource/b b
  $ hg ci -Am addb2 b
  $ hg transplant -s ../binarysource 2
  searching for changes
  applying 7a7d57e15850
  skipping emptied changeset 7a7d57e15850

Test empty result in --continue

  $ hg transplant -s ../binarysource 1
  searching for changes
  applying 645035761929
  file b already exists
  1 out of 1 hunks FAILED -- saving rejects to file b.rej
  patch failed to apply
  abort: fix up the working directory and run hg transplant --continue
  [255]
  $ hg status
  ? b.rej
  $ hg transplant --continue
  645035761929 skipped due to empty diff

  $ cd ..

Explicitly kill daemons to let the test exit on Windows

  $ killdaemons.py

Test that patch-ed files are treated as "modified", when transplant is
aborted by failure of patching, even if none of mode, size and
timestamp of them isn't changed on the filesystem (see also issue4583)

  $ cd t

  $ cat > $TESTTMP/abort.py <<EOF
  > # emulate that patch.patch() is aborted at patching on "abort" file
  > from mercurial import extensions, patch as patchmod
  > def patch(orig, ui, repo, patchname,
  >           strip=1, prefix='', files=None,
  >           eolmode='strict', similarity=0):
  >     if files is None:
  >         files = set()
  >     r = orig(ui, repo, patchname,
  >         strip=strip, prefix=prefix, files=files,
  >         eolmode=eolmode, similarity=similarity)
  >     if 'abort' in files:
  >         raise patchmod.PatchError('intentional error while patching')
  >     return r
  > def extsetup(ui):
  >     extensions.wrapfunction(patchmod, 'patch', patch)
  > EOF

  $ echo X1 > r1
  $ hg diff --nodates r1
  diff -r a53251cdf717 r1
  --- a/r1
  +++ b/r1
  @@ -1,1 +1,1 @@
  -r1
  +X1
  $ hg commit -m "X1 as r1"

  $ echo 'marking to abort patching' > abort
  $ hg add abort
  $ echo Y1 > r1
  $ hg diff --nodates r1
  diff -r 22c515968f13 r1
  --- a/r1
  +++ b/r1
  @@ -1,1 +1,1 @@
  -X1
  +Y1
  $ hg commit -m "Y1 as r1"

  $ hg update -q -C d11e3596cc1a
  $ cat r1
  r1

  $ cat >> .hg/hgrc <<EOF
  > [fakedirstatewritetime]
  > # emulate invoking dirstate.write() via repo.status() or markcommitted()
  > # at 2000-01-01 00:00
  > fakenow = 200001010000
  > 
  > # emulate invoking patch.internalpatch() at 2000-01-01 00:00
  > [fakepatchtime]
  > fakenow = 200001010000
  > 
  > [extensions]
  > fakedirstatewritetime = $TESTDIR/fakedirstatewritetime.py
  > fakepatchtime = $TESTDIR/fakepatchtime.py
  > abort = $TESTTMP/abort.py
  > EOF
  $ hg transplant "22c515968f13::"
  applying 22c515968f13
  22c515968f13 transplanted to * (glob)
  applying e38700ba9dd3
  intentional error while patching
  abort: fix up the working directory and run hg transplant --continue
  [255]
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > fakedirstatewritetime = !
  > fakepatchtime = !
  > [extensions]
  > abort = !
  > EOF

  $ cat r1
  Y1
  $ hg debugstate | grep ' r1$'
  n 644          3 unset               r1
  $ hg status -A r1
  M r1

Test that rollback by unexpected failure after transplanting the first
revision restores dirstate correctly.

  $ hg rollback -q
  $ rm -f abort
  $ hg update -q -C d11e3596cc1a
  $ hg parents -T "{node|short}\n"
  d11e3596cc1a
  $ hg status -A
  C r1
  C r2

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > # emulate failure at transplanting the 2nd revision
  > pretxncommit.abort = test ! -f abort
  > EOF
  $ hg transplant "22c515968f13::"
  applying 22c515968f13
  22c515968f13 transplanted to * (glob)
  applying e38700ba9dd3
  transaction abort!
  rollback completed
  abort: pretxncommit.abort hook exited with status 1
  [255]
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > pretxncommit.abort = !
  > EOF

  $ hg parents -T "{node|short}\n"
  d11e3596cc1a
  $ hg status -A
  M r1
  ? abort
  C r2

  $ cd ..
