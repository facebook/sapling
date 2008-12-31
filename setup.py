#!/usr/bin/env python
#
# This is the mercurial setup script.
#
# 'python setup.py install', or
# 'python setup.py --help' for more options

import sys
if not hasattr(sys, 'version_info') or sys.version_info < (2, 3, 0, 'final'):
    raise SystemExit("Mercurial requires python 2.3 or later.")

# Solaris Python packaging brain damage
try:
    import hashlib
    sha = hashlib.sha1()
except:
    try:
        import sha
    except:
        raise SystemExit(
            "Couldn't import standard hashlib (incomplete Python install).")

try:
    import zlib
except:
    raise SystemExit(
        "Couldn't import standard zlib (incomplete Python install).")

import os
import shutil
import tempfile
from distutils.core import setup, Extension
from distutils.command.install_data import install_data
from distutils.ccompiler import new_compiler

import mercurial.version

extra = {}
scripts = ['hg']
if os.name == 'nt':
    scripts.append('contrib/win32/hg.bat')

# simplified version of distutils.ccompiler.CCompiler.has_function
# that actually removes its temporary files.
def has_function(cc, funcname):
    tmpdir = tempfile.mkdtemp(prefix='hg-install-')
    devnull = oldstderr = None
    try:
        try:
            fname = os.path.join(tmpdir, 'funcname.c')
            f = open(fname, 'w')
            f.write('int main(void) {\n')
            f.write('    %s();\n' % funcname)
            f.write('}\n')
            f.close()
            # Redirect stderr to /dev/null to hide any error messages
            # from the compiler.
            # This will have to be changed if we ever have to check
            # for a function on Windows.
            devnull = open('/dev/null', 'w')
            oldstderr = os.dup(sys.stderr.fileno())
            os.dup2(devnull.fileno(), sys.stderr.fileno())
            objects = cc.compile([fname])
            cc.link_executable(objects, os.path.join(tmpdir, "a.out"))
        except:
            return False
        return True
    finally:
        if oldstderr is not None:
            os.dup2(oldstderr, sys.stderr.fileno())
        if devnull is not None:
            devnull.close()
        shutil.rmtree(tmpdir)

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

ext_modules=[
    Extension('mercurial.base85', ['mercurial/base85.c']),
    Extension('mercurial.bdiff', ['mercurial/bdiff.c']),
    Extension('mercurial.diffhelpers', ['mercurial/diffhelpers.c']),
    Extension('mercurial.mpatch', ['mercurial/mpatch.c']),
    Extension('mercurial.parsers', ['mercurial/parsers.c']),
    ]

packages = ['mercurial', 'mercurial.hgweb', 'hgext', 'hgext.convert',
            'hgext.highlight', 'hgext.zeroconf', ]

try:
    import msvcrt
    ext_modules.append(Extension('mercurial.osutil', ['mercurial/osutil.c']))
except ImportError:
    pass

try:
    import posix
    ext_modules.append(Extension('mercurial.osutil', ['mercurial/osutil.c']))

    if sys.platform == 'linux2' and os.uname()[2] > '2.6':
        # The inotify extension is only usable with Linux 2.6 kernels.
        # You also need a reasonably recent C library.
        cc = new_compiler()
        if has_function(cc, 'inotify_add_watch'):
            ext_modules.append(Extension('hgext.inotify.linux._inotify',
                                         ['hgext/inotify/linux/_inotify.c']))
            packages.extend(['hgext.inotify', 'hgext.inotify.linux'])
except ImportError:
    pass

setup(name='mercurial',
      version=mercurial.version.get_version(),
      author='Matt Mackall',
      author_email='mpm@selenic.com',
      url='http://selenic.com/mercurial',
      description='Scalable distributed SCM',
      license='GNU GPL',
      scripts=scripts,
      packages=packages,
      ext_modules=ext_modules,
      data_files=[(os.path.join('mercurial', root),
                   [os.path.join(root, file_) for file_ in files])
                  for root, dirs, files in os.walk('templates')],
      cmdclass=cmdclass,
      options=dict(py2exe=dict(packages=['hgext', 'email']),
                   bdist_mpkg=dict(zipdist=True,
                                   license='COPYING',
                                   readme='contrib/macosx/Readme.html',
                                   welcome='contrib/macosx/Welcome.html')),
      **extra)
