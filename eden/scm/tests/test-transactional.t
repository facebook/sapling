#require no-eden

  $ eagerepo

  $ cat > $TESTTMP/transactionalext.py <<'EOF'
  > from sapling import error, registrar, util
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > class failingtransaction(util.transactional):
  >     def __init__(self, closeex=None):
  >         self._closeex = closeex
  > 
  >     def close(self):
  >         if self._closeex is not None:
  >             raise self._closeex
  > 
  >     def release(self):
  >         raise FileNotFoundError("journal.dirstate")
  > 
  >     def running(self):
  >         return True
  > 
  > @command('transactional-body-abort', [], norepo=True)
  > def transactionalbodyabort(ui):
  >     with failingtransaction():
  >         raise error.Abort("primary abort")
  > 
  > @command('transactional-close-abort', [], norepo=True)
  > def transactionalcloseabort(ui):
  >     with failingtransaction(error.Abort("close abort")):
  >         pass
  > EOF

The primary transaction body exception is reported when release also fails.

  $ sl --config extensions.transactionalext=$TESTTMP/transactionalext.py transactional-body-abort
  abort: primary abort
  [255]

The close exception is reported when release also fails.

  $ sl --config extensions.transactionalext=$TESTTMP/transactionalext.py transactional-close-abort
  abort: close abort
  [255]
