
  $ LD_LIBRARY_PATH="`echo $TESTDIR/../build/lib*`"
  $ export LD_LIBRARY_PATH
  $ PYTHONPATH="`echo $TESTDIR/../build/lib*`:$PYTHONPATH"
  $ export PYTHONPATH

  $ python $TESTDIR/cstore-datapackstore.py
  $ python $TESTDIR/cstore-uniondatapackstore.py
