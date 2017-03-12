#require py3exe

This test helps in keeping a track on which commands we can run on
Python 3 and see what kind of errors are coming up.
The full traceback is hidden to have a stable output.

  $ for cmd in version debuginstall ; do
  >   echo $cmd
  >   $PYTHON3 `which hg` $cmd 2>&1 2>&1 | tail -1
  > done
  version
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
  debuginstall
  no problems detected

  $ cat > included-hgrc <<EOF
  > [extensions]
  > babar = imaginary_elephant
  > EOF
  $ cat >> $HGRCPATH <<EOF
  > %include $TESTTMP/included-hgrc
  > EOF
  $ $PYTHON3 `which hg` version | tail -1
  *** failed to import extension babar from imaginary_elephant: *: 'imaginary_elephant' (glob)
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

Test bytes-ness of policy.policy with HGMODULEPOLICY

  $ HGMODULEPOLICY=py
  $ export HGMODULEPOLICY
  $ $PYTHON3 `which hg` debuginstall 2>&1 2>&1 | tail -1
  no problems detected
