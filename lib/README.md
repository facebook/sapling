lib
===

Any native code (C/C++/Rust) that Mercurial (either core or extensions)
depends on should go here. Python code, or native code that depends on
Python code (e.g. `#import <Python.h>`) is disallowed.

As we start to convert more of Mercurial into Rust, we'll want to limit the
scope of our dependency on Python and allow end-to-end Rust code, which is why
this barrier exists.

See also `hgext/extlib/README.md`, `mercurial/cext/README.mb`.

How do I choose between `lib` and `extlib` (and `cext`)?
--------------------------------------------------------

If your code is native and doesn't depend on Python (awesome!), it goes here.

Otherwise, put it in `hgext/extlib` (if it's only used by extensions) or
`mercurial/cext` (if it's used by extensions or core).
