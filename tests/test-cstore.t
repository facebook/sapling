Get build path for native extensions
- Do this as a separate script so we can eat the ls errors
  $ cat >> $TESTTMP/getbuildpath.sh <<EOF
  > BUILD_PATH="`ls -d $TESTDIR/`"
  > BUILD_PATH="\$BUILD_PATH:`ls -d $TESTDIR/../build/lib* 2> /dev/null`"
  > BUILD_PATH="\$BUILD_PATH:`ls -d $TESTDIR/../../rpmbuild/BUILD/fb-mercurial-ext-*/build/lib.*/ 2> /dev/null`"
  > echo "\$BUILD_PATH"
  > EOF
  $ chmod a+x $TESTTMP/getbuildpath.sh
  $ BUILD_PATH="`$TESTTMP/getbuildpath.sh`"

  $ LD_LIBRARY_PATH="$BUILD_PATH"
  $ export LD_LIBRARY_PATH
  $ PYTHONPATH="$BUILD_PATH:$PYTHONPATH"
  $ export PYTHONPATH

  $ python $TESTDIR/remotefilelog-datapack.py
  $ python $TESTDIR/remotefilelog-histpack.py
  $ python $TESTDIR/cstore-datapackstore.py
  $ python $TESTDIR/cstore-treemanifest.py
  $ python $TESTDIR/cstore-uniondatapackstore.py
