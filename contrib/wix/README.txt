WiX installer source files
==========================

The files in this folder are used by the thg-winbuild [1] package
building architecture to create a Mercurial MSI installer.   These files
are versioned within the Mercurial source tree because the WXS files
must kept up to date with distribution changes within their branch.  In
other words, the default branch WXS files are expected to diverge from
the stable branch WXS files.  Storing them within the same repository is
the only sane way to keep the source tree and the installer in sync.

The MSI installer builder uses only the mercurial.ini file from the
contrib/win32 folder, the contents of which have been historically used
to create an InnoSetup based installer.  The rest of the files there are
ignored.

The MSI packages built by thg-winbuild require elevated (admin)
privileges to be installed due to the installation of MSVC CRT libraries
under the C:\WINDOWS\WinSxS folder.  Thus the InnoSetup installers may
still be useful to some users.

To build your own MSI packages, clone the thg-winbuild [1] repository
and follow the README.txt [2] instructions closely.  There are fewer
prerequisites for a WiX [3] installer than an InnoSetup installer, but
they are more specific.

Direct questions or comments to Steve Borho <steve@borho.org>

[1] http://bitbucket.org/tortoisehg/thg-winbuild
[2] http://bitbucket.org/tortoisehg/thg-winbuild/src/tip/README.txt
[3] http://wix.sourceforge.net/
