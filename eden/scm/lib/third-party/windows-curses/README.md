Python curses wheels for Windows
================================

![Latest Version](https://img.shields.io/pypi/v/windows-curses)
![Supported Python Implementations](https://img.shields.io/pypi/implementation/windows-curses)

This is the repository for the [windows-curses wheels on
PyPI](https://pypi.org/project/windows-curses). The wheels are based on the
[wheels on Christoph Gohlke's
page](https://www.lfd.uci.edu/~gohlke/pythonlibs/#curses).

Only `build-wheels.bat` is original work.

Wheels built from this repository can be installed with this command:

    pip install windows-curses

Starting with version 2.0, these wheels include a hack to make resizing work
for Python applications that haven't been specifically adapted for PDCurses.
See [this
commit](https://github.com/zephyrproject-rtos/windows-curses/commit/30ca08bfbcb7a332228ddcde026181b2009ea0a7).
The description on PyPI has a longer explanation.

Note that this hack is not in Gohlke's wheels.

Maintainers Wanted
------------------

This project is not actively maintained and is looking for maintainers.

If you are interested, please let us know by either creating an issue here or messaging in the
[#windows-support channel on Zephyr Discord](https://discord.gg/ygfnbCZCtU).

Background
----------

The `curses` module is in the Python standard library, but is not available on
Windows. Trying to import `curses` gives an import error for `_curses`, which
is provided by `Modules/_cursesmodule.c` in the CPython source code.

The wheels provided here are based on patches from
https://bugs.python.org/issue2889, which make minor modifications to
`_cursesmodule.c` to make it compatible with Windows and the
[PDCurses](https://pdcurses.sourceforge.io) curses implementation.  `setup.py`
defines `HAVE_*` macros for features available in PDCurses and makes some minor
additional compatibility tweaks.

The patched `_cursesmodule.c` is linked against PDCurses to produce a wheel
that provides the `_curses` module on Windows and allows the standard `curses`
module to run.

Unicode support
---------------

The wheels are built with wide character support and force the encoding to
UTF-8. Remove `UTF8=y` from the `nmake` line in `build-wheels.bat` to use the
default system encoding instead.

Build instructions
------------------

 1. Clone the repository with the following command:

        git clone --recurse-submodules https://github.com/zephyrproject-rtos/windows-curses.git

    `--recurse-submodules` pulls in the required PDCurses Git submodule.

 2. Install compilers compatible with the Python versions that you want to
    builds wheel for by following the instructions at
    https://wiki.python.org/moin/WindowsCompilers.

    Visual Studio 2019 will work for Python 3.6-3.9.

    Visual Studio 2022 will work for Python 3.10-3.11.

 3. Install Python 3.6 or later to get
    the [Python launcher for Windows](https://docs.python.org/3/using/windows.html#launcher).

 4. Install any other Python versions you want to build wheels for.

    Only the Python X.Y versions that have `pyXY\` directories are supported.

 5. Install/upgrade the `wheel` and `setuptools` packages for all Python
    versions. Taking Python 3.6 as an example, the following command will do
    it:

        py -3.6 -m pip install --upgrade wheel setuptools

    `py` is the Python launcher, which makes it easy to run a particular Python
    version.

 6. Open the Visual Studio
    [Developer Command Prompt](https://docs.microsoft.com/en-us/dotnet/framework/tools/developer-command-prompt-for-vs)
    of the compiler required by the version of Python that you want to build
    a wheel for.

    Use the 32-bit version (`x86 Native Tools Command Prompt for VS 2022`) to build wheels for 32-bit
    Python versions, and the 64-bit version (e.g.
    `x64 Native Tools Command Prompt for VS 2022`) to build wheels for 64-bit Python versions.

 7. Run `build-wheels.bat`, passing it the Python version you're building a
    wheel for. For example, the following command will build a wheel for
    Python 3.6:

        build-wheels.bat 3.6

    If you have both 32-bit and 64-bit versions of the same Python version
    installed and are building a 32-bit wheel, add "-32" to the version
    number, like in the following example:

        build-wheels.bat 3.6-32

    If you are building multiple wheels for Python versions that are all
    compatible with the same compiler, you can list all of them in the same
    command:

        build-wheels.bat 3.6 3.7

    `build-wheels.bat` first cleans and rebuilds PDCurses, and then builds and
    links the source code in `pyXY\` for each of the specified Python versions,
    producing wheels as output in `dist\`.

### Rebuilding the wheels for Python 3.6, 3.7, 3.8, 3.9, 3.10, and 3.11

In `x86 Native Tools Command Prompt for VS 2022`:

    build-wheels.bat 3.6-32 3.7-32 3.8-32 3.9-32 3.10-32 3.11-32

In `x64 Native Tools Command Prompt for VS 2022`:

    build-wheels.bat 3.6 3.7 3.8 3.9 3.10 3.11


This gives a set of wheels in `dist\`.

Compatibility note
------------------

This building scheme above should be the safest one to use. In practice, many
of the resulting wheels seem to be forwards- and backwards-compatible.

Making a new release
--------------------

  1. Bump the version number in `setup.py` according to the [Semantic versioning](https://semver.org/).

  2. Create a Git tag for the release:

         git tag -s -m "windows-curses 1.2.3" v1.2.3
         git push upstream v1.2.3

     For pre-releases, add `aNUMBER` after the release name (e.g. `v1.2.3a1`, `v1.2.3a2`, ...).

  3. [Create a GitHub release](https://github.com/zephyrproject-rtos/windows-curses/releases/new)
     from the tag.

     The name of the GitHub release should match the name of the release tag (e.g. `v1.2.3`) and its
     body should contain a brief release note.

Once a GitHub release is created, the GitHub Actions CI will automatically build and upload the
wheels to the PyPI.

Uploading to PyPI
-----------------

**NOTE: The process of uploading wheels for releases is automated using the GitHub Actions and
manual uploads should not be necessary under normal circumstances.**

Don't forget to bump the version number in `setup.py` before building new
wheels. [Semantic versioning](https://semver.org/) is intended.

Once the wheels are built, follow the instructions
[here](https://packaging.python.org/tutorials/distributing-packages/#uploading-your-project-to-pypi)
to upload them to PyPI.

`pip`/PyPI will look at the wheel metadata and automatically install the right
version of the wheel.

Adding support for a new Python version
---------------------------------------

1. Create a new directory for the Python version, e.g. `py39\`

2. Copy `Modules\_cursesmodule.c` from the CPython source code to `py39\_cursesmodule.c`

3. Apply the PDCurses compatibility patch from [this commit](https://github.com/zephyrproject-rtos/windows-curses/commit/b1cf4e10cecb9ba3e43766407c2ed2b138571f85) and the resizing hack from [this commit](https://github.com/zephyrproject-rtos/windows-curses/commit/30ca08bfbcb7a332228ddcde026181b2009ea0a7) to the new `py39\_cursesmodule.c`.

4. Copy `Modules\_curses_panel.c`, `Modules\clinic\_cursesmodule.c.h`, and `Modules\clinic\_curses_panel.c.h` from the CPython sources to `py39\_curses_panel.c`, `py39\clinic\_cursesmodule.c.h` and `py39\clinic\_curses_panel.c.h`, respectively

5. Add the build specifications for the new Python version in `.github/workflows/ci.yml`.

In practise, `Modules\_cursesmodule.c` from newer Python 3 versions is likely to be compatible with older Python 3 versions too. The Python 3.6 and 3.7 wheels are currently built from identical `_cursesmodule.c` files (but not the Python 3.8 or 3.9 wheels).

For Python 3.10 and 3.11 it is necessary to adapt `_cursesmodule.c` and `clinic\_cursesmodule.c.h` files to new Python API (decribed more here https://devguide.python.org/c-api). It demands removing two headers files as described in [this commit]().
