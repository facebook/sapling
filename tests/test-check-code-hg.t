
  $ check_code="$TESTDIR"/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ EXTRAS=`python -c 'import lz4revlog' 2> /dev/null && echo "--config extensions.lz4revlog="`
  $ hg $EXTRAS locate | sed 's-\\-/-g' |
  >   xargs "$check_code" --warnings --per-file=0 || false
  Skipping CMakeLists.txt it has no-che?k-code (glob)
  Skipping remotefilelog/cdatapack/buffer.h it has no-che?k-code (glob)
  Skipping remotefilelog/cdatapack/cdatapack.c it has no-che?k-code (glob)
  Skipping remotefilelog/cdatapack/cdatapack.h it has no-che?k-code (glob)
  Skipping remotefilelog/cdatapack/cdatapack_dump.c it has no-che?k-code (glob)
  Skipping remotefilelog/cdatapack/cdatapack_get.c it has no-che?k-code (glob)
  Skipping remotefilelog/cdatapack/convert.h it has no-che?k-code (glob)
  Skipping remotefilelog/cdatapack/null_test.c it has no-che?k-code (glob)
  Skipping remotefilelog/cdatapack/py-cdatapack.c it has no-che?k-code (glob)
  Skipping remotefilelog/ctreemanifest/convert.h it has no-che?k-code (glob)
  Skipping remotefilelog/ctreemanifest/manifest.h it has no-che?k-code (glob)
  Skipping remotefilelog/ctreemanifest/manifest_entry.h it has no-che?k-code (glob)
  Skipping remotefilelog/ctreemanifest/manifest_fetcher.h it has no-che?k-code (glob)
  Skipping remotefilelog/ctreemanifest/pythonutil.h it has no-che?k-code (glob)
  Skipping tests/test-bad-configs.t it has no-che?k-code (glob)
