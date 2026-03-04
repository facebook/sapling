#require python3 xargs

#testcases rust python

Test that biggrep integration works correctly and that Python and Rust
implementations pass the same arguments and produce the same output.

#if python
  $ setconfig grep.use-rust=false
#else
  $ setconfig grep.use-rust=true
#endif

Set up the repository with files that match fake-biggrep-client.py:
  $ newclientrepo
  $ mkdir -p grepdir/subdir1 grepdir/subdir2
  $ echo 'foobarbaz' > grepdir/grepfile1
  $ echo 'foobarboo' > grepdir/grepfile2
  $ printf '%s\n' '-g' > grepdir/grepfile3
  $ echo 'foobar_subdir' > grepdir/subdir1/subfile1
  $ echo 'foobar_dirsub' > grepdir/subdir2/subfile2
  $ hg add grepdir
  adding grepdir/grepfile1
  adding grepdir/grepfile2
  adding grepdir/grepfile3
  adding grepdir/subdir1/subfile1
  adding grepdir/subdir2/subfile2
  $ hg commit -m "Initial commit"
  $ COMMIT1=$(hg log -r . -T'{node}')

  $ setconfig grep.biggrepclient=$TESTDIR/fake-biggrep-client.py
  $ setconfig grep.usebiggrep=True
  $ setconfig grep.biggrepcorpus=fake
  $ setconfig grep.biggreptier=test.tier

Define the biggrep files JSON for tests:
  $ BGFILES='{"grepdir/grepfile1": "foobarbaz", "grepdir/grepfile2": "foobarboo", "grepdir/grepfile3": "-g", "grepdir/subdir1/subfile1": "foobar_subdir", "grepdir/subdir2/subfile2": "foobar_dirsub"}'
  $ export BIGGREP_FILES="$BGFILES"

Helper to capture biggrep args:
  $ capture_args() {
  >   BIGGREP_ARGS_FILE="$TESTTMP/bg_args" hg grep --color=off "$@" >/dev/null 2>&1 || true
  >   cat "$TESTTMP/bg_args"
  > }

Test basic argument passing:
  $ capture_args -n foobar
  test.tier fake re2 --stripdir -r --expression foobar

Test with -i (ignore case):
  $ capture_args -i foobar
  test.tier fake re2 --stripdir -r --expression foobar -i

Test with -l (files with matches):
  $ capture_args -l foobar
  test.tier fake re2 --stripdir -r --expression foobar -l

Test with context options:
  $ capture_args -A 2 foobar
  test.tier fake re2 --stripdir -r --expression foobar -A 2
  $ capture_args -B 3 foobar
  test.tier fake re2 --stripdir -r --expression foobar -B 3
  $ capture_args -C 1 foobar
  test.tier fake re2 --stripdir -r --expression foobar -C 1

Test dash escaping in pattern:
  $ capture_args -- -g
  test.tier fake re2 --stripdir -r --expression \-g

Test with file pattern scoping:
  $ capture_args foobar grepdir/subdir1
  test.tier fake re2 --stripdir -r --expression foobar -f (grepdir/subdir1)

Test from subdirectory (cwd scoping):
  $ cd grepdir/subdir1
  $ capture_args foobar
  test.tier fake re2 --stripdir -r --expression foobar -f (grepdir/subdir1)
  $ cd ../..

Now test output formatting:

Test basic output:
  $ hg grep --color=off foobar | sort
  grepdir/grepfile1:foobarbaz_bg
  grepdir/grepfile2:foobarboo_bg
  grepdir/subdir1/subfile1:foobar_subdir_bg
  grepdir/subdir2/subfile2:foobar_dirsub_bg

Test output with line numbers:
  $ hg grep --color=off -n foobar | sort
  grepdir/grepfile1:1:foobarbaz_bg
  grepdir/grepfile2:1:foobarboo_bg
  grepdir/subdir1/subfile1:1:foobar_subdir_bg
  grepdir/subdir2/subfile2:1:foobar_dirsub_bg

Test filtered output with file pattern:
  $ hg grep --color=off foobar grepdir/subdir1 | sort
  grepdir/subdir1/subfile1:foobar_subdir_bg

Test dash escaping produces correct match:
  $ hg grep --color=off -- -g | sort
  grepdir/grepfile3:-g_bg

Test from subdirectory with relative paths:
  $ cd grepdir/subdir1
  $ hg grep --color=off foobar | sort
  subfile1:foobar_subdir_bg
  $ hg grep --color=off -n foobar ../subdir2 | sort
  ../subdir2/subfile2:1:foobar_dirsub_bg
  $ cd ../..

Test manifest diff - corpus is older than working directory:

Create a second commit with changes:
  $ echo 'newfoobar' > grepdir/newfile
  $ echo 'modified_foobar' > grepdir/grepfile1
  $ hg rm grepdir/grepfile2
  $ hg add grepdir/newfile
  $ hg commit -m "Second commit: add newfile, modify grepfile1, remove grepfile2"

Set corpus to the first commit - biggrep returns stale data:
  $ BIGGREP_CORPUS_REV='.^' hg grep --color=off foobar | sort
  grepdir/grepfile1:modified_foobar
  grepdir/newfile:newfoobar
  grepdir/subdir1/subfile1:foobar_subdir_bg
  grepdir/subdir2/subfile2:foobar_dirsub_bg

The output should include:
- grepfile1: grepped locally (modified since corpus) - no _bg suffix
- grepfile2: excluded (removed since corpus) - not in output
- newfile: grepped locally (added since corpus) - no _bg suffix
- subdir1/subfile1 and subdir2/subfile2: from biggrep (unchanged) - has _bg suffix

Test with line numbers:
  $ BIGGREP_CORPUS_REV='.^' hg grep --color=off -n foobar | sort
  grepdir/grepfile1:1:modified_foobar
  grepdir/newfile:1:newfoobar
  grepdir/subdir1/subfile1:1:foobar_subdir_bg
  grepdir/subdir2/subfile2:1:foobar_dirsub_bg

Test manifest diff - corpus is newer than working directory:
Go back to the first commit, but corpus points to second commit.
  $ hg goto '.^' -q

Define BIGGREP_FILES for second commit state:
  $ BGFILES_V2='{"grepdir/grepfile1": "modified_foobar", "grepdir/grepfile3": "-g", "grepdir/newfile": "newfoobar", "grepdir/subdir1/subfile1": "foobar_subdir", "grepdir/subdir2/subfile2": "foobar_dirsub"}'
  $ export BGFILES_V2

  $ BIGGREP_FILES="$BGFILES_V2" BIGGREP_CORPUS_REV='max(all())' hg grep --color=off foobar | sort
  grepdir/grepfile1:foobarbaz
  grepdir/grepfile2:foobarboo
  grepdir/subdir1/subfile1:foobar_subdir_bg
  grepdir/subdir2/subfile2:foobar_dirsub_bg

The output should include:
- grepfile1: grepped locally (different in corpus vs wdir) - no _bg suffix
- grepfile2: grepped locally (exists in wdir but not corpus) - no _bg suffix
- newfile: excluded (exists in corpus but not wdir)
- subdir1/subfile1 and subdir2/subfile2: from biggrep (unchanged) - has _bg suffix

Go back to the second commit:
  $ hg goto 'max(all())' -q

Test manifest diff with uncommitted changes:
Make some uncommitted modifications:
  $ echo 'uncommitted_foobar' > grepdir/grepfile1
  $ echo 'new_uncommitted_foobar' > grepdir/uncommitted_file
  $ hg add grepdir/uncommitted_file
  $ hg rm grepdir/grepfile3

Corpus points to current commit (second commit), but we have uncommitted changes:
  $ BIGGREP_FILES="$BGFILES_V2" hg grep --color=off foobar | sort
  grepdir/grepfile1:uncommitted_foobar
  grepdir/newfile:newfoobar_bg
  grepdir/subdir1/subfile1:foobar_subdir_bg
  grepdir/subdir2/subfile2:foobar_dirsub_bg
  grepdir/uncommitted_file:new_uncommitted_foobar

The output should include:
- grepfile1: grepped locally (uncommitted modification) - no _bg suffix
- grepfile3: excluded (uncommitted removal)
- newfile: from biggrep (unchanged from corpus) - has _bg suffix
- uncommitted_file: grepped locally (uncommitted add) - no _bg suffix
- subdir1/subfile1 and subdir2/subfile2: from biggrep (unchanged) - has _bg suffix

Clean up uncommitted changes:
  $ hg revert --all -q
  $ rm grepdir/uncommitted_file

#if rust
Test manifest diff with --rev to search an older revision (Rust only):
When searching an older rev, the diff is between corpus and that rev.

Search the first commit when corpus points to current commit:
  $ BIGGREP_FILES="$BGFILES_V2" BIGGREP_CORPUS_REV='.' hg grep --color=off -r "$COMMIT1" foobar | sort
  grepdir/grepfile1:foobarbaz
  grepdir/grepfile2:foobarboo
  grepdir/subdir1/subfile1:foobar_subdir_bg
  grepdir/subdir2/subfile2:foobar_dirsub_bg

The output should include:
- grepfile1: grepped locally (different in target rev) - no _bg suffix
- grepfile2: grepped locally (exists in target but not corpus) - no _bg suffix
- newfile: excluded (doesn't exist in target rev)
- subdir1/subfile1 and subdir2/subfile2: from biggrep (unchanged) - has _bg suffix
#endif
