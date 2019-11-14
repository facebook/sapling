  $ . helpers-usechg.sh

#require svn svn-bindings

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert =
  > EOF

  $ svnadmin create svn-repo
  $ svnadmin load -q svn-repo < "$TESTDIR/svn/move.svndump"
  $ SVNREPOPATH=`pwd`/svn-repo
#if windows
  $ SVNREPOURL=file:///`$PYTHON -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$SVNREPOPATH"`
#else
  $ SVNREPOURL=file://`$PYTHON -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$SVNREPOPATH"`
#endif

Convert trunk and branches

  $ hg convert --datesort "$SVNREPOURL"/subproject A-hg
  initializing destination A-hg repository
  scanning source...
  sorting...
  converting...
  13 createtrunk
  12 moved1
  11 moved1
  10 moved2
  9 changeb and rm d2
  8 changeb and rm d2
  7 moved1again
  6 moved1again
  5 copyfilefrompast
  4 copydirfrompast
  3 add d3
  2 copy dir and remove subdir
  1 add d4old
  0 rename d4old into d4new

  $ cd A-hg
  $ hg log -G --template '{rev} {desc|firstline} files: {files}\n'
  o  13 rename d4old into d4new files: d4new/g d4old/g
  |
  o  12 add d4old files: d4old/g
  |
  o  11 copy dir and remove subdir files: d3/d31/e d4/d31/e d4/f
  |
  o  10 add d3 files: d3/d31/e d3/f
  |
  o  9 copydirfrompast files: d2/d
  |
  o  8 copyfilefrompast files: d
  |
  o  7 moved1again files: d1/b d1/c
  |
  | o  6 moved1again files:
  | |
  o |  5 changeb and rm d2 files: d1/b d2/d
  | |
  | o  4 changeb and rm d2 files: b
  | |
  o |  3 moved2 files: d2/d
  | |
  o |  2 moved1 files: d1/b d1/c
  | |
  | o  1 moved1 files: b c
  |
  o  0 createtrunk files:
  

Check move copy records

  $ hg st --rev 12:13 --copies
  A d4new/g
    d4old/g
  R d4old/g

Check branches

  $ hg log -r 'all()' -T '{extras}\n' | sed 's/convert_revision=.*//' | sort -u
  branch=d1
  branch=default
  $ cd ..

  $ mkdir test-replace
  $ cd test-replace
  $ svnadmin create svn-repo
  $ svnadmin load -q svn-repo < "$TESTDIR/svn/replace.svndump"

Convert files being replaced by directories

  $ hg convert svn-repo hg-repo
  initializing destination hg-repo repository
  scanning source...
  sorting...
  converting...
  6 initial
  5 clobber symlink
  4 clobber1
  3 clobber2
  2 adddb
  1 clobberdir
  0 branch

  $ cd hg-repo

Manifest before

  $ hg -v manifest -r 1
  644   a
  644 @ dlink
  644 @ dlink2
  644   dlink3
  644   d/b
  644   d2/a

Manifest after clobber1

  $ hg -v manifest -r 2
  644 @ dlink2
  644   dlink3
  644   a/b
  644   d/b
  644   d2/a
  644   dlink/b

Manifest after clobber2

  $ hg -v manifest -r 3
  644 @ dlink2
  644 @ dlink3
  644   a/b
  644   d/b
  644   d2/a
  644   dlink/b

Manifest after clobberdir

  $ hg -v manifest -r 6
  644 @ dlink2
  644 @ dlink3
  644   a/b
  644   d/b
  644   d2/a
  644   d2/c
  644   dlink/b

Try updating

  $ hg up -qC default
  $ cd ..

Test convert progress bar

  $ cat >> $HGRCPATH <<EOF
  > [progress]
  > debug = true
  > EOF

  $ hg convert svn-repo hg-progress
  initializing destination hg-progress repository
  scanning source...
  progress: scanning: 1/7 revisions (14.29%)
  progress: scanning: 2/7 revisions (28.57%)
  progress: scanning: 3/7 revisions (42.86%)
  progress: scanning: 4/7 revisions (57.14%)
  progress: scanning: 5/7 revisions (71.43%)
  progress: scanning: 6/7 revisions (85.71%)
  progress: scanning: 7/7 revisions (100.00%)
  progress: scanning (end)
  sorting...
  converting...
  6 initial
  progress: converting: 0/7 revisions (0.00%)
  progress: getting files: a 1/6 files (16.67%)
  progress: getting files: d/b 2/6 files (33.33%)
  progress: getting files: d2/a 3/6 files (50.00%)
  progress: getting files: dlink 4/6 files (66.67%)
  progress: getting files: dlink2 5/6 files (83.33%)
  progress: getting files: dlink3 6/6 files (100.00%)
  progress: getting files (end)
  5 clobber symlink
  progress: converting: 1/7 revisions (14.29%)
  progress: scanning paths: /trunk/dlink3 0/1 paths (0.00%)
  progress: scanning paths (end)
  progress: getting files: dlink3 1/1 files (100.00%)
  progress: getting files (end)
  4 clobber1
  progress: converting: 2/7 revisions (28.57%)
  progress: scanning paths: /trunk/a 0/2 paths (0.00%)
  progress: scanning paths: /trunk/dlink 1/2 paths (50.00%)
  progress: scanning paths (end)
  progress: getting files: a 1/4 files (25.00%)
  progress: getting files: dlink 2/4 files (50.00%)
  progress: getting files: a/b 3/4 files (75.00%)
  progress: getting files: dlink/b 4/4 files (100.00%)
  progress: getting files (end)
  3 clobber2
  progress: converting: 3/7 revisions (42.86%)
  progress: scanning paths: /trunk/dlink3 0/1 paths (0.00%)
  progress: scanning paths (end)
  progress: getting files: dlink3 1/1 files (100.00%)
  progress: getting files (end)
  2 adddb
  progress: converting: 4/7 revisions (57.14%)
  progress: scanning paths: /trunk/d2/b 0/1 paths (0.00%)
  progress: scanning paths (end)
  progress: getting files: d2/b 1/1 files (100.00%)
  progress: getting files (end)
  1 clobberdir
  progress: converting: 5/7 revisions (71.43%)
  progress: scanning paths: /trunk/d2 0/1 paths (0.00%)
  progress: scanning paths (end)
  progress: getting files: a/b 1/8 files (12.50%)
  progress: getting files: d/b 2/8 files (25.00%)
  progress: getting files: d2/a 3/8 files (37.50%)
  progress: getting files: d2/b 4/8 files (50.00%)
  progress: getting files: dlink/b 5/8 files (62.50%)
  progress: getting files: dlink2 6/8 files (75.00%)
  progress: getting files: dlink3 7/8 files (87.50%)
  progress: getting files: d2/c 8/8 files (100.00%)
  progress: getting files (end)
  0 branch
  progress: converting: 6/7 revisions (85.71%)
  progress: scanning paths: /branches/branch 0/3 paths (0.00%)
  progress: scanning paths: /branches/branch/d2/b 1/3 paths (33.33%)
  progress: scanning paths: /branches/branch/d2/c 2/3 paths (66.67%)
  progress: scanning paths (end)
  progress: getting files: a/b 1/8 files (12.50%)
  progress: getting files: d/b 2/8 files (25.00%)
  progress: getting files: d2/a 3/8 files (37.50%)
  progress: getting files: d2/b 4/8 files (50.00%)
  progress: getting files: dlink/b 5/8 files (62.50%)
  progress: getting files: dlink2 6/8 files (75.00%)
  progress: getting files: dlink3 7/8 files (87.50%)
  progress: getting files: d2/c 8/8 files (100.00%)
  progress: getting files (end)
  progress: converting (end)

  $ cd ..
