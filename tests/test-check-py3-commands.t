#require py3exe

This test helps in keeping a track on which commands we can run on
Python 3 and see what kind of errors are coming up.
The full traceback is hidden to have a stable output.

  $ for cmd in version debuginstall ; do
  >   echo $cmd
  >   $PYTHON3 `which hg` $cmd 2>&1 2>&1 | tail -1
  > done
  version
  TypeError: startswith first arg must be str or a tuple of str, not bytes
  debuginstall
  TypeError: startswith first arg must be str or a tuple of str, not bytes
