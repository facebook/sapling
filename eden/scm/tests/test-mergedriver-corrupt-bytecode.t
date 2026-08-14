#require no-eden

  $ enable mergedriver rebase
  $ newclientrepo

Create a package-style merge driver with a corrupt cache for one of its imports.

  $ mkdir $TESTTMP/driver
  $ cat > $TESTTMP/driver/__init__.py << 'EOF'
  > from .privacylib import preprocess
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     pass
  > EOF
  $ cat > $TESTTMP/driver/privacylib.py << 'EOF'
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     ui.write("merge driver loaded from source\n")
  > EOF

  >>> import importlib.util, os, py_compile
  >>> source = os.path.join(os.environ["TESTTMP"], "driver", "privacylib.py")
  >>> cache = importlib.util.cache_from_source(source)
  >>> _ = py_compile.compile(source, cfile=cache, doraise=True)
  >>> with open(cache, "r+b") as f:
  ...     _ = f.seek(16)
  ...     _ = f.write(b"\x00")

  $ setconfig experimental.mergedriver=python:$TESTTMP/driver

Force an in-memory three-way merge so the merge driver loads during rebase.

  $ drawdag << 'EOS'
  > B  # B/FILE = destination\n2\n3\n
  > |
  > | C # C/FILE = 1\n2\nsource\n
  > |/
  > A  # A/FILE = 1\n2\n3\n
  > EOS

FIXME: Sapling should discard the corrupt cache and load the merge driver from
source.

  $ sl rebase -q -r $C -d $B 2>/dev/null
  loading preprocess hook failed: bad marshal data (unknown type code)
  [1]

#if no-windows
Recreate the corrupt cache in a directory that Sapling cannot modify.

  >>> _ = py_compile.compile(source, cfile=cache, doraise=True)
  >>> with open(cache, "r+b") as f:
  ...     _ = f.seek(16)
  ...     _ = f.write(b"\x00")
  >>> os.chmod(os.path.dirname(cache), 0o555)

  $ newclientrepo permission
  $ setconfig experimental.mergedriver=python:$TESTTMP/driver
  $ drawdag << 'EOS'
  > B  # B/FILE = destination\n2\n3\n
  > |
  > | C # C/FILE = 1\n2\nsource\n
  > |/
  > A  # A/FILE = 1\n2\n3\n
  > EOS

  $ sl rebase -q -r $C -d $B 2>/dev/null
  loading preprocess hook failed: bad marshal data (unknown type code)
  [1]
  $ chmod u+w $TESTTMP/driver/__pycache__
#endif
