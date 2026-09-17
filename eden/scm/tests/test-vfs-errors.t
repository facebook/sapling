Rust VFS errors must match Python's built-in exception classes.

  >>> import errno
  >>> import os
  >>> import bindings
  >>> from sapling import vfs
  >>> native = bindings.io.vfs(os.getcwd())
  >>> python = vfs.vfs(os.getcwd())
  >>> for operation in (native.metadata, native.open, native.read, python.stat, python.lstat, python.open, python.read):
  ...     for path in ("missing", "missing/child"):
  ...         try:
  ...             operation(path)
  ...         except FileNotFoundError as error:
  ...             assert type(error) is FileNotFoundError
  ...             assert error.errno == errno.ENOENT
  ...             assert os.path.basename(error.filename) in ("missing", "child")
  ...         else:
  ...             raise AssertionError("missing path did not raise FileNotFoundError")

An existing directory must not be mistaken for a missing path.

  >>> native.mkdir("existing")
  >>> try:
  ...     native.mkdir("existing")
  ... except FileExistsError as error:
  ...     assert error.errno == errno.EEXIST
  ... else:
  ...     raise AssertionError("existing directory did not raise FileExistsError")
