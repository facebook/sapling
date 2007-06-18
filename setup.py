#!/usr/bin/env python
#
# This is the mercurial setup script.
#
# './setup.py install', or
# './setup.py --help' for more options

import sys
if not hasattr(sys, 'version_info') or sys.version_info < (2, 3, 0, 'final'):
    raise SystemExit, "Mercurial requires python 2.3 or later."

import os
from distutils.core import setup, Extension
from distutils.command.install_data import install_data

import mercurial.version
import mercurial.demandimport
mercurial.demandimport.enable = lambda: None

extra = {}

# py2exe needs to be installed to work
try:
    import py2exe

    # Help py2exe to find win32com.shell
    try:
        import modulefinder
        import win32com
        for p in win32com.__path__[1:]: # Take the path to win32comext
            modulefinder.AddPackagePath("win32com", p)
        pn = "win32com.shell"
        __import__(pn)
        m = sys.modules[pn]
        for p in m.__path__[1:]:
            modulefinder.AddPackagePath(pn, p)
    except ImportError:
        pass

    extra['console'] = ['hg']

except ImportError:
    pass

# specify version string, otherwise 'hg identify' will be used:
version = ''

class install_package_data(install_data):
    def finalize_options(self):
        self.set_undefined_options('install',
                                   ('install_lib', 'install_dir'))
        install_data.finalize_options(self)

mercurial.version.remember_version(version)
cmdclass = {'install_data': install_package_data}

setup(name='mercurial',
      version=mercurial.version.get_version(),
      author='Matt Mackall',
      author_email='mpm@selenic.com',
      url='http://selenic.com/mercurial',
      description='Scalable distributed SCM',
      license='GNU GPL',
      packages=['mercurial', 'mercurial.hgweb', 'hgext', 'hgext.convert'],
      ext_modules=[Extension('mercurial.mpatch', ['mercurial/mpatch.c']),
                   Extension('mercurial.bdiff', ['mercurial/bdiff.c']),
                   Extension('mercurial.base85', ['mercurial/base85.c'])],
      data_files=[(os.path.join('mercurial', root),
                   [os.path.join(root, file_) for file_ in files])
                  for root, dirs, files in os.walk('templates')],
      cmdclass=cmdclass,
      scripts=['hg', 'hgmerge'],
      options=dict(py2exe=dict(packages=['hgext']),
                   bdist_mpkg=dict(zipdist=True,
                                   license='COPYING',
                                   readme='contrib/macosx/Readme.html',
                                   welcome='contrib/macosx/Welcome.html')),
      **extra)
