#
# This is an experimental py3k-enabled mercurial setup script.
#
# 'python setup.py install', or
# 'python setup.py --help' for more options

from distutils.command.build_py import build_py_2to3
from lib2to3.refactor import get_fixers_from_package as getfixers

import sys
if not hasattr(sys, 'version_info') or sys.version_info < (2, 4, 0, 'final'):
    raise SystemExit("Mercurial requires Python 2.4 or later.")

if sys.version_info[0] >= 3:
    def b(s):
        '''A helper function to emulate 2.6+ bytes literals using string
        literals.'''
        return s.encode('latin1')
else:
    def b(s):
        '''A helper function to emulate 2.6+ bytes literals using string
        literals.'''
        return s

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

try:
    import bz2
except:
    raise SystemExit(
        "Couldn't import standard bz2 (incomplete Python install).")

import os, subprocess, time
import shutil
import tempfile
from distutils import log
from distutils.core import setup, Extension
from distutils.dist import Distribution
from distutils.command.build import build
from distutils.command.build_ext import build_ext
from distutils.command.build_py import build_py
from distutils.spawn import spawn, find_executable
from distutils.ccompiler import new_compiler
from distutils.errors import CCompilerError

scripts = ['hg']
if os.name == 'nt':
    scripts.append('contrib/win32/hg.bat')

# simplified version of distutils.ccompiler.CCompiler.has_function
# that actually removes its temporary files.
def hasfunction(cc, funcname):
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
            objects = cc.compile([fname], output_dir=tmpdir)
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
    py2exeloaded = True

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

except ImportError:
    py2exeloaded = False
    pass

def runcmd(cmd, env):
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, env=env)
    out, err = p.communicate()
    # If root is executing setup.py, but the repository is owned by
    # another user (as in "sudo python setup.py install") we will get
    # trust warnings since the .hg/hgrc file is untrusted. That is
    # fine, we don't want to load it anyway.  Python may warn about
    # a missing __init__.py in mercurial/locale, we also ignore that.
    err = [e for e in err.splitlines()
           if not e.startswith(b('Not trusting file')) \
              and not e.startswith(b('warning: Not importing'))]
    if err:
        return ''
    return out

version = ''

if os.path.isdir('.hg'):
    # Execute hg out of this directory with a custom environment which
    # includes the pure Python modules in mercurial/pure. We also take
    # care to not use any hgrc files and do no localization.
    pypath = ['mercurial', os.path.join('mercurial', 'pure')]
    env = {'PYTHONPATH': os.pathsep.join(pypath),
           'HGRCPATH': '',
           'LANGUAGE': 'C'}
    if 'LD_LIBRARY_PATH' in os.environ:
        env['LD_LIBRARY_PATH'] = os.environ['LD_LIBRARY_PATH']
    if 'SystemRoot' in os.environ:
        # Copy SystemRoot into the custom environment for Python 2.6
        # under Windows. Otherwise, the subprocess will fail with
        # error 0xc0150004. See: http://bugs.python.org/issue3440
        env['SystemRoot'] = os.environ['SystemRoot']
    cmd = [sys.executable, 'hg', 'id', '-i', '-t']
    l = runcmd(cmd, env).split()
    while len(l) > 1 and l[-1][0].isalpha(): # remove non-numbered tags
        l.pop()
    if len(l) > 1: # tag found
        version = l[-1]
        if l[0].endswith('+'): # propagate the dirty status to the tag
            version += '+'
    elif len(l) == 1: # no tag found
        cmd = [sys.executable, 'hg', 'parents', '--template',
               '{latesttag}+{latesttagdistance}-']
        version = runcmd(cmd, env) + l[0]
    if version.endswith('+'):
        version += time.strftime('%Y%m%d')
elif os.path.exists('.hg_archival.txt'):
    kw = dict([[t.strip() for t in l.split(':', 1)]
               for l in open('.hg_archival.txt')])
    if 'tag' in kw:
        version =  kw['tag']
    elif 'latesttag' in kw:
        version = '%(latesttag)s+%(latesttagdistance)s-%(node).12s' % kw
    else:
        version = kw.get('node', '')[:12]

if version:
    f = open("mercurial/__version__.py", "w")
    f.write('# this file is autogenerated by setup.py\n')
    f.write('version = "%s"\n' % version)
    f.close()


try:
    from mercurial import __version__
    version = __version__.version
except ImportError:
    version = 'unknown'

class hgbuildmo(build):

    description = "build translations (.mo files)"

    def run(self):
        if not find_executable('msgfmt'):
            self.warn("could not find msgfmt executable, no translations "
                     "will be built")
            return

        podir = 'i18n'
        if not os.path.isdir(podir):
            self.warn("could not find %s/ directory" % podir)
            return

        join = os.path.join
        for po in os.listdir(podir):
            if not po.endswith('.po'):
                continue
            pofile = join(podir, po)
            modir = join('locale', po[:-3], 'LC_MESSAGES')
            mofile = join(modir, 'hg.mo')
            mobuildfile = join('mercurial', mofile)
            cmd = ['msgfmt', '-v', '-o', mobuildfile, pofile]
            if sys.platform != 'sunos5':
                # msgfmt on Solaris does not know about -c
                cmd.append('-c')
            self.mkpath(join('mercurial', modir))
            self.make_file([pofile], mobuildfile, spawn, (cmd,))

# Insert hgbuildmo first so that files in mercurial/locale/ are found
# when build_py is run next.
build.sub_commands.insert(0, ('build_mo', None))
# We also need build_ext before build_py. Otherwise, when 2to3 is called (in
# build_py), it will not find osutil & friends, thinking that those modules are
# global and, consequently, making a mess, now that all module imports are
# global.
build.sub_commands.insert(1, ('build_ext', None))

Distribution.pure = 0
Distribution.global_options.append(('pure', None, "use pure (slow) Python "
                                    "code instead of C extensions"))

class hgbuildext(build_ext):

    def build_extension(self, ext):
        try:
            build_ext.build_extension(self, ext)
        except CCompilerError:
            if not hasattr(ext, 'optional') or not ext.optional:
                raise
            log.warn("Failed to build optional extension '%s' (skipping)",
                     ext.name)

class hgbuildpy(build_py_2to3):
    fixer_names = sorted(set(getfixers("lib2to3.fixes") +
                             getfixers("hgfixes")))

    def finalize_options(self):
        build_py.finalize_options(self)

        if self.distribution.pure:
            if self.py_modules is None:
                self.py_modules = []
            for ext in self.distribution.ext_modules:
                if ext.name.startswith("mercurial."):
                    self.py_modules.append("mercurial.pure.%s" % ext.name[10:])
            self.distribution.ext_modules = []

    def find_modules(self):
        modules = build_py.find_modules(self)
        for module in modules:
            if module[0] == "mercurial.pure":
                if module[1] != "__init__":
                    yield ("mercurial", module[1], module[2])
            else:
                yield module

    def run(self):
        # In the build_py_2to3 class, self.updated_files = [], but I couldn't
        # see when that variable was updated to point to the updated files, as
        # its names suggests. Thus, I decided to just find_all_modules and feed
        # them to 2to3. Unfortunately, subsequent calls to setup3k.py will
        # incur in 2to3 analysis overhead.
        self.updated_files = [i[2] for i in self.find_all_modules()]

        # Base class code
        if self.py_modules:
            self.build_modules()
        if self.packages:
            self.build_packages()
            self.build_package_data()

        # 2to3
        self.run_2to3(self.updated_files)

        # Remaining base class code
        self.byte_compile(self.get_outputs(include_bytecode=0))

cmdclass = {'build_mo': hgbuildmo,
            'build_ext': hgbuildext,
            'build_py': hgbuildpy}

packages = ['mercurial', 'mercurial.hgweb', 'hgext', 'hgext.convert',
            'hgext.highlight', 'hgext.zeroconf']

pymodules = []

extmodules = [
    Extension('mercurial.base85', ['mercurial/base85.c']),
    Extension('mercurial.bdiff', ['mercurial/bdiff.c']),
    Extension('mercurial.diffhelpers', ['mercurial/diffhelpers.c']),
    Extension('mercurial.mpatch', ['mercurial/mpatch.c']),
    Extension('mercurial.parsers', ['mercurial/parsers.c']),
    ]

# disable osutil.c under windows + python 2.4 (issue1364)
if sys.platform == 'win32' and sys.version_info < (2, 5, 0, 'final'):
    pymodules.append('mercurial.pure.osutil')
else:
    extmodules.append(Extension('mercurial.osutil', ['mercurial/osutil.c']))

if sys.platform == 'linux2' and os.uname()[2] > '2.6':
    # The inotify extension is only usable with Linux 2.6 kernels.
    # You also need a reasonably recent C library.
    # In any case, if it fails to build the error will be skipped ('optional').
    cc = new_compiler()
    if hasfunction(cc, 'inotify_add_watch'):
        inotify = Extension('hgext.inotify.linux._inotify',
                            ['hgext/inotify/linux/_inotify.c'],
                            ['mercurial'])
        inotify.optional = True
        extmodules.append(inotify)
        packages.extend(['hgext.inotify', 'hgext.inotify.linux'])

packagedata = {'mercurial': ['locale/*/LC_MESSAGES/hg.mo',
                             'help/*.txt']}

def ordinarypath(p):
    return p and p[0] != '.' and p[-1] != '~'

for root in ('templates',):
    for curdir, dirs, files in os.walk(os.path.join('mercurial', root)):
        curdir = curdir.split(os.sep, 1)[1]
        dirs[:] = filter(ordinarypath, dirs)
        for f in filter(ordinarypath, files):
            f = os.path.join(curdir, f)
            packagedata['mercurial'].append(f)

datafiles = []
setupversion = version
extra = {}

if py2exeloaded:
    extra['console'] = [
        {'script':'hg',
         'copyright':'Copyright (C) 2005-2010 Matt Mackall and others',
         'product_version':version}]

if os.name == 'nt':
    # Windows binary file versions for exe/dll files must have the
    # form W.X.Y.Z, where W,X,Y,Z are numbers in the range 0..65535
    setupversion = version.split('+', 1)[0]

setup(name='mercurial',
      version=setupversion,
      author='Matt Mackall',
      author_email='mpm@selenic.com',
      url='http://mercurial.selenic.com/',
      description='Scalable distributed SCM',
      license='GNU GPLv2+',
      scripts=scripts,
      packages=packages,
      py_modules=pymodules,
      ext_modules=extmodules,
      data_files=datafiles,
      package_data=packagedata,
      cmdclass=cmdclass,
      options=dict(py2exe=dict(packages=['hgext', 'email']),
                   bdist_mpkg=dict(zipdist=True,
                                   license='COPYING',
                                   readme='contrib/macosx/Readme.html',
                                   welcome='contrib/macosx/Welcome.html')),
      **extra)
