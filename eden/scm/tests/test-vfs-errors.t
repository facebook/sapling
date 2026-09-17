Rust VFS errors must match Python's built-in exception classes.

FIXME: Before Python 3.11, FileNotFoundError handlers miss these exceptions
even though their values are FileNotFoundError instances.

  >>> import errno
  >>> import os
  >>> import sys
  >>> import bindings
  >>> from sapling import vfs
  >>> native = bindings.io.vfs(os.getcwd())
  >>> python = vfs.vfs(os.getcwd())
  >>> for operation in (native.metadata, native.open, native.read, python.stat, python.lstat, python.open, python.read):
  ...     for path in ("missing", "missing/child"):
  ...         try:
  ...             operation(path)
  ...         except FileNotFoundError as error:
  ...             caught, matched = error, True
  ...         except OSError as error:
  ...             caught, matched = error, False
  ...         else:
  ...             raise AssertionError("missing path did not raise FileNotFoundError")
  ...         assert matched == (sys.version_info >= (3, 11))
  ...         assert type(caught) is FileNotFoundError
  ...         assert caught.errno == errno.ENOENT
  ...         assert os.path.basename(caught.filename) in ("missing", "child")

An existing directory must not be mistaken for a missing path.

FIXME: Windows exposes ERROR_ALREADY_EXISTS (183) as errno, producing OSError.
Before Python 3.11, FileExistsError handlers also miss non-Windows exceptions.

  >>> native.mkdir("existing")
  >>> try:
  ...     native.mkdir("existing")
  ... except FileExistsError as error:
  ...     caught, matched = error, True
  ... except OSError as error:
  ...     caught, matched = error, False
  ... else:
  ...     raise AssertionError("existing directory did not raise FileExistsError")
  >>> assert matched == (sys.version_info >= (3, 11) and os.name != "nt")
  >>> assert type(caught) is (OSError if os.name == "nt" else FileExistsError)
  >>> assert caught.errno == (183 if os.name == "nt" else errno.EEXIST)
