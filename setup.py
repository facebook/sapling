#
# This is the mercurial setup script.
#
# 'python setup.py install', or
# 'python setup.py --help' for more options

import sys, platform
if getattr(sys, 'version_info', (0, 0, 0)) < (2, 4, 0, 'final'):
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

# The base IronPython distribution (as of 2.7.1) doesn't support bz2
isironpython = False
try:
    isironpython = platform.python_implementation().lower().find("ironpython") != -1
except:
    pass

if isironpython:
    print "warning: IronPython detected (no bz2 support)"
else:
    try:
        import bz2
    except:
        raise SystemExit(
            "Couldn't import standard bz2 (incomplete Python install).")

import os, subprocess, time
import shutil
import tempfile
from distutils import log
from distutils.core import setup, Command, Extension
from distutils.dist import Distribution
from distutils.command.build import build
from distutils.command.build_ext import build_ext
from distutils.command.build_py import build_py
from distutils.command.install_scripts import install_scripts
from distutils.spawn import spawn, find_executable
from distutils.ccompiler import new_compiler
from distutils.errors import CCompilerError, DistutilsExecError
from distutils.sysconfig import get_python_inc
from distutils.version import StrictVersion

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
except ImportError:
    py2exeloaded = False

def runcmd(cmd, env):
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, env=env)
    out, err = p.communicate()
    return out, err

def runhg(cmd, env):
    out, err = runcmd(cmd, env)
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

if os.path.isdir('.hg'):
    cmd = [sys.executable, 'hg', 'id', '-i', '-t']
    l = runhg(cmd, env).split()
    while len(l) > 1 and l[-1][0].isalpha(): # remove non-numbered tags
        l.pop()
    if len(l) > 1: # tag found
        version = l[-1]
        if l[0].endswith('+'): # propagate the dirty status to the tag
            version += '+'
    elif len(l) == 1: # no tag found
        cmd = [sys.executable, 'hg', 'parents', '--template',
               '{latesttag}+{latesttagdistance}-']
        version = runhg(cmd, env) + l[0]
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

Distribution.pure = 0
Distribution.global_options.append(('pure', None, "use pure (slow) Python "
                                    "code instead of C extensions"))

class hgbuildext(build_ext):

    def build_extension(self, ext):
        try:
            build_ext.build_extension(self, ext)
        except CCompilerError:
            if not getattr(ext, 'optional', False):
                raise
            log.warn("Failed to build optional extension '%s' (skipping)",
                     ext.name)

class hgbuildpy(build_py):

    def finalize_options(self):
        build_py.finalize_options(self)

        if self.distribution.pure:
            if self.py_modules is None:
                self.py_modules = []
            for ext in self.distribution.ext_modules:
                if ext.name.startswith("mercurial."):
                    self.py_modules.append("mercurial.pure.%s" % ext.name[10:])
            self.distribution.ext_modules = []
        else:
            if not os.path.exists(os.path.join(get_python_inc(), 'Python.h')):
                raise SystemExit("Python headers are required to build Mercurial")

    def find_modules(self):
        modules = build_py.find_modules(self)
        for module in modules:
            if module[0] == "mercurial.pure":
                if module[1] != "__init__":
                    yield ("mercurial", module[1], module[2])
            else:
                yield module

class buildhgextindex(Command):
    description = 'generate prebuilt index of hgext (for frozen package)'
    user_options = []
    _indexfilename = 'hgext/__index__.py'

    def initialize_options(self):
        pass

    def finalize_options(self):
        pass

    def run(self):
        if os.path.exists(self._indexfilename):
            os.unlink(self._indexfilename)

        # here no extension enabled, disabled() lists up everything
        code = ('import pprint; from mercurial import extensions; '
                'pprint.pprint(extensions.disabled())')
        out, err = runcmd([sys.executable, '-c', code], env)
        if err:
            raise DistutilsExecError(err)

        f = open(self._indexfilename, 'w')
        f.write('# this file is autogenerated by setup.py\n')
        f.write('docs = ')
        f.write(out)
        f.close()

class hginstallscripts(install_scripts):
    '''
    This is a specialization of install_scripts that replaces the @LIBDIR@ with
    the configured directory for modules. If possible, the path is made relative
    to the directory for scripts.
    '''

    def initialize_options(self):
        install_scripts.initialize_options(self)

        self.install_lib = None

    def finalize_options(self):
        install_scripts.finalize_options(self)
        self.set_undefined_options('install',
                                   ('install_lib', 'install_lib'))

    def run(self):
        install_scripts.run(self)

        if (os.path.splitdrive(self.install_dir)[0] !=
            os.path.splitdrive(self.install_lib)[0]):
            # can't make relative paths from one drive to another, so use an
            # absolute path instead
            libdir = self.install_lib
        else:
            common = os.path.commonprefix((self.install_dir, self.install_lib))
            rest = self.install_dir[len(common):]
            uplevel = len([n for n in os.path.split(rest) if n])

            libdir =  uplevel * ('..' + os.sep) + self.install_lib[len(common):]

        for outfile in self.outfiles:
            fp = open(outfile, 'rb')
            data = fp.read()
            fp.close()

            # skip binary files
            if '\0' in data:
                continue

            data = data.replace('@LIBDIR@', libdir.encode('string_escape'))
            fp = open(outfile, 'wb')
            fp.write(data)
            fp.close()

cmdclass = {'build_mo': hgbuildmo,
            'build_ext': hgbuildext,
            'build_py': hgbuildpy,
            'build_hgextindex': buildhgextindex,
            'install_scripts': hginstallscripts}

packages = ['mercurial', 'mercurial.hgweb',
            'mercurial.httpclient', 'mercurial.httpclient.tests',
            'hgext', 'hgext.convert', 'hgext.highlight', 'hgext.zeroconf',
            'hgext.largefiles']

pymodules = []

extmodules = [
    Extension('mercurial.base85', ['mercurial/base85.c']),
    Extension('mercurial.bdiff', ['mercurial/bdiff.c']),
    Extension('mercurial.diffhelpers', ['mercurial/diffhelpers.c']),
    Extension('mercurial.mpatch', ['mercurial/mpatch.c']),
    Extension('mercurial.parsers', ['mercurial/parsers.c']),
    ]

osutil_ldflags = []

if sys.platform == 'darwin':
    osutil_ldflags += ['-framework', 'ApplicationServices']

# disable osutil.c under windows + python 2.4 (issue1364)
if sys.platform == 'win32' and sys.version_info < (2, 5, 0, 'final'):
    pymodules.append('mercurial.pure.osutil')
else:
    extmodules.append(Extension('mercurial.osutil', ['mercurial/osutil.c'],
                                extra_link_args=osutil_ldflags))

if sys.platform.startswith('linux') and os.uname()[2] > '2.6':
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
    # sub command of 'build' because 'py2exe' does not handle sub_commands
    build.sub_commands.insert(0, ('build_hgextindex', None))

if os.name == 'nt':
    # Windows binary file versions for exe/dll files must have the
    # form W.X.Y.Z, where W,X,Y,Z are numbers in the range 0..65535
    setupversion = version.split('+', 1)[0]

if sys.platform == 'darwin' and os.path.exists('/usr/bin/xcodebuild'):
    # XCode 4.0 dropped support for ppc architecture, which is hardcoded in
    # distutils.sysconfig
    version = runcmd(['/usr/bin/xcodebuild', '-version'], {})[0].splitlines()[0]
    # Also parse only first digit, because 3.2.1 can't be parsed nicely
    if (version.startswith('Xcode') and
        StrictVersion(version.split()[1]) >= StrictVersion('4.0')):
        os.environ['ARCHFLAGS'] = ''

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
