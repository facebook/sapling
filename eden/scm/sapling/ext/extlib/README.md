extlib
======

Code that extensions depend on, but aren't themselves extensions, should go here.
Both native (C/C++/Cython/Rust) and Python code is allowed. Code that depends on Python
is also allowed.

In theory, this code should slowly disappear as extension code gets folded into
mainline Mercurial. (The native bits should go into `lib/` or `mercurial/cext`),
the Python code into `mercurial/` itself.)

See also `lib/README.md`, `mercurial/cext/README.md`.
