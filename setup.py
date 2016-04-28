#
# This is the mercurial setup script.
#
# 'python setup.py install', or
# 'python setup.py --help' for more options

import sys, platform
if getattr(sys, 'version_info', (0, 0, 0)) < (2, 6, 0, 'final'):
    raise SystemExit("Mercurial requires Python 2.6 or later.")

if sys.version_info[0] >= 3:
    printf = eval('print')
    libdir_escape = 'unicode_escape'
else:
    libdir_escape = 'string_escape'
    def printf(*args, **kwargs):
        f = kwargs.get('file', sys.stdout)
        end = kwargs.get('end', '\n')
        f.write(b' '.join(args) + end)

# Solaris Python packaging brain damage
try:
    import hashlib
    sha = hashlib.sha1()
except ImportError:
    try:
        import sha
        sha.sha # silence unused import warning
    except ImportError:
        raise SystemExit(
            "Couldn't import standard hashlib (incomplete Python install).")

try:
    import zlib
    zlib.compressobj # silence unused import warning
except ImportError:
    raise SystemExit(
        "Couldn't import standard zlib (incomplete Python install).")

# The base IronPython distribution (as of 2.7.1) doesn't support bz2
isironpython = False
try:
    isironpython = (platform.python_implementation()
                    .lower().find("ironpython") != -1)
except AttributeError:
    pass

if isironpython:
    sys.stderr.write("warning: IronPython detected (no bz2 support)\n")
else:
    try:
        import bz2
        bz2.BZ2Compressor # silence unused import warning
    except ImportError:
        raise SystemExit(
            "Couldn't import standard bz2 (incomplete Python install).")

ispypy = "PyPy" in sys.version

import ctypes
import os, stat, subprocess, time
import re
import shutil
import tempfile
from distutils import log
if 'FORCE_SETUPTOOLS' in os.environ:
    from setuptools import setup
else:
    from distutils.core import setup
from distutils.core import Command, Extension
from distutils.dist import Distribution
from distutils.command.build import build
from distutils.command.build_ext import build_ext
from distutils.command.build_py import build_py
from distutils.command.build_scripts import build_scripts
from distutils.command.install_lib import install_lib
from distutils.command.install_scripts import install_scripts
from distutils.spawn import spawn, find_executable
from distutils import file_util
from distutils.errors import (
    CCompilerError,
    DistutilsError,
    DistutilsExecError,
)
from distutils.sysconfig import get_python_inc, get_config_var
from distutils.version import StrictVersion

scripts = ['hg']
if os.name == 'nt':
    # We remove hg.bat if we are able to build hg.exe.
    scripts.append('contrib/win32/hg.bat')

# simplified version of distutils.ccompiler.CCompiler.has_function
# that actually removes its temporary files.
def hasfunction(cc, funcname):
    tmpdir = tempfile.mkdtemp(prefix='hg-install-')
    devnull = oldstderr = None
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
        return True
    except Exception:
        return False
    finally:
        if oldstderr is not None:
            os.dup2(oldstderr, sys.stderr.fileno())
        if devnull is not None:
            devnull.close()
        shutil.rmtree(tmpdir)

# py2exe needs to be installed to work
try:
    import py2exe
    py2exe.Distribution # silence unused import warning
    py2exeloaded = True
    # import py2exe's patched Distribution class
    from distutils.core import Distribution
except ImportError:
    py2exeloaded = False

def runcmd(cmd, env):
    if (sys.platform == 'plan9'
       and (sys.version_info[0] == 2 and sys.version_info[1] < 7)):
        # subprocess kludge to work around issues in half-baked Python
        # ports, notably bichued/python:
        _, out, err = os.popen3(cmd)
        return str(out), str(err)
    else:
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
           if not e.startswith(b'not trusting file') \
              and not e.startswith(b'warning: Not importing') \
              and not e.startswith(b'obsolete feature not enabled')]
    if err:
        printf("stderr from '%s':" % (' '.join(cmd)), file=sys.stderr)
        printf(b'\n'.join([b'  ' + e for e in err]), file=sys.stderr)
        return ''
    return out

version = ''

# Execute hg out of this directory with a custom environment which takes care
# to not use any hgrc files and do no localization.
env = {'HGMODULEPOLICY': 'py',
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
    cmd = [sys.executable, 'hg', 'log', '-r', '.', '--template', '{tags}\n']
    numerictags = [t for t in runhg(cmd, env).split() if t[0].isdigit()]
    hgid = runhg([sys.executable, 'hg', 'id', '-i'], env).strip()
    if numerictags: # tag(s) found
        version = numerictags[-1]
        if hgid.endswith('+'): # propagate the dirty status to the tag
            version += '+'
    else: # no tag found
        ltagcmd = [sys.executable, 'hg', 'parents', '--template',
                   '{latesttag}']
        ltag = runhg(ltagcmd, env)
        changessincecmd = [sys.executable, 'hg', 'log', '-T', 'x\n', '-r',
                           "only(.,'%s')" % ltag]
        changessince = len(runhg(changessincecmd, env).splitlines())
        version = '%s+%s-%s' % (ltag, changessince, hgid)
    if version.endswith('+'):
        version += time.strftime('%Y%m%d')
elif os.path.exists('.hg_archival.txt'):
    kw = dict([[t.strip() for t in l.split(':', 1)]
               for l in open('.hg_archival.txt')])
    if 'tag' in kw:
        version = kw['tag']
    elif 'latesttag' in kw:
        if 'changessincelatesttag' in kw:
            version = '%(latesttag)s+%(changessincelatesttag)s-%(node).12s' % kw
        else:
            version = '%(latesttag)s+%(latesttagdistance)s-%(node).12s' % kw
    else:
        version = kw.get('node', '')[:12]

if version:
    with open("mercurial/__version__.py", "w") as f:
        f.write('# this file is autogenerated by setup.py\n')
        f.write('version = "%s"\n' % version)

try:
    oldpolicy = os.environ.get('HGMODULEPOLICY', None)
    os.environ['HGMODULEPOLICY'] = 'py'
    from mercurial import __version__
    version = __version__.version
except ImportError:
    version = 'unknown'
finally:
    if oldpolicy is None:
        del os.environ['HGMODULEPOLICY']
    else:
        os.environ['HGMODULEPOLICY'] = oldpolicy

class hgbuild(build):
    # Insert hgbuildmo first so that files in mercurial/locale/ are found
    # when build_py is run next.
    sub_commands = [('build_mo', None)] + build.sub_commands

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


class hgdist(Distribution):
    pure = ispypy

    global_options = Distribution.global_options + \
                     [('pure', None, "use pure (slow) Python "
                        "code instead of C extensions"),
                     ]

    def has_ext_modules(self):
        # self.ext_modules is emptied in hgbuildpy.finalize_options which is
        # too late for some cases
        return not self.pure and Distribution.has_ext_modules(self)

class hgbuildext(build_ext):

    def build_extension(self, ext):
        try:
            build_ext.build_extension(self, ext)
        except CCompilerError:
            if not getattr(ext, 'optional', False):
                raise
            log.warn("Failed to build optional extension '%s' (skipping)",
                     ext.name)

class hgbuildscripts(build_scripts):
    def run(self):
        if os.name != 'nt' or self.distribution.pure:
            return build_scripts.run(self)

        exebuilt = False
        try:
            self.run_command('build_hgexe')
            exebuilt = True
        except (DistutilsError, CCompilerError):
            log.warn('failed to build optional hg.exe')

        if exebuilt:
            # Copying hg.exe to the scripts build directory ensures it is
            # installed by the install_scripts command.
            hgexecommand = self.get_finalized_command('build_hgexe')
            dest = os.path.join(self.build_dir, 'hg.exe')
            self.mkpath(self.build_dir)
            self.copy_file(hgexecommand.hgexepath, dest)

            # Remove hg.bat because it is redundant with hg.exe.
            self.scripts.remove('contrib/win32/hg.bat')

        return build_scripts.run(self)

class hgbuildpy(build_py):
    def finalize_options(self):
        build_py.finalize_options(self)

        if self.distribution.pure:
            self.distribution.ext_modules = []
        else:
            h = os.path.join(get_python_inc(), 'Python.h')
            if not os.path.exists(h):
                raise SystemExit('Python headers are required to build '
                                 'Mercurial but weren\'t found in %s' % h)

    def run(self):
        if self.distribution.pure:
            modulepolicy = 'py'
        else:
            modulepolicy = 'c'
        with open("mercurial/__modulepolicy__.py", "w") as f:
            f.write('# this file is autogenerated by setup.py\n')
            f.write('modulepolicy = "%s"\n' % modulepolicy)

        build_py.run(self)

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
            with open(self._indexfilename, 'w') as f:
                f.write('# empty\n')

        # here no extension enabled, disabled() lists up everything
        code = ('import pprint; from mercurial import extensions; '
                'pprint.pprint(extensions.disabled())')
        out, err = runcmd([sys.executable, '-c', code], env)
        if err:
            raise DistutilsExecError(err)

        with open(self._indexfilename, 'w') as f:
            f.write('# this file is autogenerated by setup.py\n')
            f.write('docs = ')
            f.write(out)

class buildhgexe(build_ext):
    description = 'compile hg.exe from mercurial/exewrapper.c'

    def build_extensions(self):
        if os.name != 'nt':
            return
        if isinstance(self.compiler, HackedMingw32CCompiler):
            self.compiler.compiler_so = self.compiler.compiler # no -mdll
            self.compiler.dll_libraries = [] # no -lmsrvc90

        # Different Python installs can have different Python library
        # names. e.g. the official CPython distribution uses pythonXY.dll
        # and MinGW uses libpythonX.Y.dll.
        _kernel32 = ctypes.windll.kernel32
        _kernel32.GetModuleFileNameA.argtypes = [ctypes.c_void_p,
                                                 ctypes.c_void_p,
                                                 ctypes.c_ulong]
        _kernel32.GetModuleFileNameA.restype = ctypes.c_ulong
        size = 1000
        buf = ctypes.create_string_buffer(size + 1)
        filelen = _kernel32.GetModuleFileNameA(sys.dllhandle, ctypes.byref(buf),
                                               size)

        if filelen > 0 and filelen != size:
            dllbasename = os.path.basename(buf.value)
            if not dllbasename.lower().endswith('.dll'):
                raise SystemExit('Python DLL does not end with .dll: %s' %
                                 dllbasename)
            pythonlib = dllbasename[:-4]
        else:
            log.warn('could not determine Python DLL filename; '
                     'assuming pythonXY')

            hv = sys.hexversion
            pythonlib = 'python%d%d' % (hv >> 24, (hv >> 16) & 0xff)

        log.info('using %s as Python library name' % pythonlib)
        with open('mercurial/hgpythonlib.h', 'wb') as f:
            f.write('/* this file is autogenerated by setup.py */\n')
            f.write('#define HGPYTHONLIB "%s"\n' % pythonlib)
        objects = self.compiler.compile(['mercurial/exewrapper.c'],
                                         output_dir=self.build_temp)
        dir = os.path.dirname(self.get_ext_fullpath('dummy'))
        target = os.path.join(dir, 'hg')
        self.compiler.link_executable(objects, target,
                                      libraries=[],
                                      output_dir=self.build_temp)

    @property
    def hgexepath(self):
        dir = os.path.dirname(self.get_ext_fullpath('dummy'))
        return os.path.join(self.build_temp, dir, 'hg.exe')

class hginstalllib(install_lib):
    '''
    This is a specialization of install_lib that replaces the copy_file used
    there so that it supports setting the mode of files after copying them,
    instead of just preserving the mode that the files originally had.  If your
    system has a umask of something like 027, preserving the permissions when
    copying will lead to a broken install.

    Note that just passing keep_permissions=False to copy_file would be
    insufficient, as it might still be applying a umask.
    '''

    def run(self):
        realcopyfile = file_util.copy_file
        def copyfileandsetmode(*args, **kwargs):
            src, dst = args[0], args[1]
            dst, copied = realcopyfile(*args, **kwargs)
            if copied:
                st = os.stat(src)
                # Persist executable bit (apply it to group and other if user
                # has it)
                if st[stat.ST_MODE] & stat.S_IXUSR:
                    setmode = int('0755', 8)
                else:
                    setmode = int('0644', 8)
                m = stat.S_IMODE(st[stat.ST_MODE])
                m = (m & ~int('0777', 8)) | setmode
                os.chmod(dst, m)
        file_util.copy_file = copyfileandsetmode
        try:
            install_lib.run(self)
        finally:
            file_util.copy_file = realcopyfile

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

        # It only makes sense to replace @LIBDIR@ with the install path if
        # the install path is known. For wheels, the logic below calculates
        # the libdir to be "../..". This is because the internal layout of a
        # wheel archive looks like:
        #
        #   mercurial-3.6.1.data/scripts/hg
        #   mercurial/__init__.py
        #
        # When installing wheels, the subdirectories of the "<pkg>.data"
        # directory are translated to system local paths and files therein
        # are copied in place. The mercurial/* files are installed into the
        # site-packages directory. However, the site-packages directory
        # isn't known until wheel install time. This means we have no clue
        # at wheel generation time what the installed site-packages directory
        # will be. And, wheels don't appear to provide the ability to register
        # custom code to run during wheel installation. This all means that
        # we can't reliably set the libdir in wheels: the default behavior
        # of looking in sys.path must do.

        if (os.path.splitdrive(self.install_dir)[0] !=
            os.path.splitdrive(self.install_lib)[0]):
            # can't make relative paths from one drive to another, so use an
            # absolute path instead
            libdir = self.install_lib
        else:
            common = os.path.commonprefix((self.install_dir, self.install_lib))
            rest = self.install_dir[len(common):]
            uplevel = len([n for n in os.path.split(rest) if n])

            libdir = uplevel * ('..' + os.sep) + self.install_lib[len(common):]

        for outfile in self.outfiles:
            with open(outfile, 'rb') as fp:
                data = fp.read()

            # skip binary files
            if b'\0' in data:
                continue

            # During local installs, the shebang will be rewritten to the final
            # install path. During wheel packaging, the shebang has a special
            # value.
            if data.startswith(b'#!python'):
                log.info('not rewriting @LIBDIR@ in %s because install path '
                         'not known' % outfile)
                continue

            data = data.replace(b'@LIBDIR@', libdir.encode(libdir_escape))
            with open(outfile, 'wb') as fp:
                fp.write(data)

cmdclass = {'build': hgbuild,
            'build_mo': hgbuildmo,
            'build_ext': hgbuildext,
            'build_py': hgbuildpy,
            'build_scripts': hgbuildscripts,
            'build_hgextindex': buildhgextindex,
            'install_lib': hginstalllib,
            'install_scripts': hginstallscripts,
            'build_hgexe': buildhgexe,
            }

packages = ['mercurial', 'mercurial.hgweb', 'mercurial.httpclient',
            'mercurial.pure',
            'hgext', 'hgext.convert', 'hgext.fsmonitor',
            'hgext.fsmonitor.pywatchman', 'hgext.highlight',
            'hgext.largefiles', 'hgext.zeroconf', 'hgext3rd']

common_depends = ['mercurial/util.h']

osutil_ldflags = []

if sys.platform == 'darwin':
    osutil_ldflags += ['-framework', 'ApplicationServices']

extmodules = [
    Extension('mercurial.base85', ['mercurial/base85.c'],
              depends=common_depends),
    Extension('mercurial.bdiff', ['mercurial/bdiff.c'],
              depends=common_depends),
    Extension('mercurial.diffhelpers', ['mercurial/diffhelpers.c'],
              depends=common_depends),
    Extension('mercurial.mpatch', ['mercurial/mpatch.c'],
              depends=common_depends),
    Extension('mercurial.parsers', ['mercurial/dirs.c',
                                    'mercurial/manifest.c',
                                    'mercurial/parsers.c',
                                    'mercurial/pathencode.c'],
              depends=common_depends),
    Extension('mercurial.osutil', ['mercurial/osutil.c'],
              extra_link_args=osutil_ldflags,
              depends=common_depends),
    Extension('hgext.fsmonitor.pywatchman.bser',
              ['hgext/fsmonitor/pywatchman/bser.c']),
    ]

try:
    from distutils import cygwinccompiler

    # the -mno-cygwin option has been deprecated for years
    compiler = cygwinccompiler.Mingw32CCompiler

    class HackedMingw32CCompiler(cygwinccompiler.Mingw32CCompiler):
        def __init__(self, *args, **kwargs):
            compiler.__init__(self, *args, **kwargs)
            for i in 'compiler compiler_so linker_exe linker_so'.split():
                try:
                    getattr(self, i).remove('-mno-cygwin')
                except ValueError:
                    pass

    cygwinccompiler.Mingw32CCompiler = HackedMingw32CCompiler
except ImportError:
    # the cygwinccompiler package is not available on some Python
    # distributions like the ones from the optware project for Synology
    # DiskStation boxes
    class HackedMingw32CCompiler(object):
        pass

packagedata = {'mercurial': ['locale/*/LC_MESSAGES/hg.mo',
                             'help/*.txt',
                             'help/internals/*.txt',
                             'default.d/*.rc',
                             'dummycert.pem']}

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
         'copyright':'Copyright (C) 2005-2016 Matt Mackall and others',
         'product_version':version}]
    # sub command of 'build' because 'py2exe' does not handle sub_commands
    build.sub_commands.insert(0, ('build_hgextindex', None))
    # put dlls in sub directory so that they won't pollute PATH
    extra['zipfile'] = 'lib/library.zip'

if os.name == 'nt':
    # Windows binary file versions for exe/dll files must have the
    # form W.X.Y.Z, where W,X,Y,Z are numbers in the range 0..65535
    setupversion = version.split('+', 1)[0]

if sys.platform == 'darwin' and os.path.exists('/usr/bin/xcodebuild'):
    version = runcmd(['/usr/bin/xcodebuild', '-version'], {})[0].splitlines()
    if version:
        version = version[0]
        if sys.version_info[0] == 3:
            version = version.decode('utf-8')
        xcode4 = (version.startswith('Xcode') and
                  StrictVersion(version.split()[1]) >= StrictVersion('4.0'))
        xcode51 = re.match(r'^Xcode\s+5\.1', version) is not None
    else:
        # xcodebuild returns empty on OS X Lion with XCode 4.3 not
        # installed, but instead with only command-line tools. Assume
        # that only happens on >= Lion, thus no PPC support.
        xcode4 = True
        xcode51 = False

    # XCode 4.0 dropped support for ppc architecture, which is hardcoded in
    # distutils.sysconfig
    if xcode4:
        os.environ['ARCHFLAGS'] = ''

    # XCode 5.1 changes clang such that it now fails to compile if the
    # -mno-fused-madd flag is passed, but the version of Python shipped with
    # OS X 10.9 Mavericks includes this flag. This causes problems in all
    # C extension modules, and a bug has been filed upstream at
    # http://bugs.python.org/issue21244. We also need to patch this here
    # so Mercurial can continue to compile in the meantime.
    if xcode51:
        cflags = get_config_var('CFLAGS')
        if cflags and re.search(r'-mno-fused-madd\b', cflags) is not None:
            os.environ['CFLAGS'] = (
                os.environ.get('CFLAGS', '') + ' -Qunused-arguments')

setup(name='mercurial',
      version=setupversion,
      author='Matt Mackall and many others',
      author_email='mercurial@selenic.com',
      url='https://mercurial-scm.org/',
      download_url='https://mercurial-scm.org/release/',
      description=('Fast scalable distributed SCM (revision control, version '
                   'control) system'),
      long_description=('Mercurial is a distributed SCM tool written in Python.'
                        ' It is used by a number of large projects that require'
                        ' fast, reliable distributed revision control, such as '
                        'Mozilla.'),
      license='GNU GPLv2 or any later version',
      classifiers=[
          'Development Status :: 6 - Mature',
          'Environment :: Console',
          'Intended Audience :: Developers',
          'Intended Audience :: System Administrators',
          'License :: OSI Approved :: GNU General Public License (GPL)',
          'Natural Language :: Danish',
          'Natural Language :: English',
          'Natural Language :: German',
          'Natural Language :: Italian',
          'Natural Language :: Japanese',
          'Natural Language :: Portuguese (Brazilian)',
          'Operating System :: Microsoft :: Windows',
          'Operating System :: OS Independent',
          'Operating System :: POSIX',
          'Programming Language :: C',
          'Programming Language :: Python',
          'Topic :: Software Development :: Version Control',
      ],
      scripts=scripts,
      packages=packages,
      ext_modules=extmodules,
      data_files=datafiles,
      package_data=packagedata,
      cmdclass=cmdclass,
      distclass=hgdist,
      options={'py2exe': {'packages': ['hgext', 'email']},
               'bdist_mpkg': {'zipdist': False,
                              'license': 'COPYING',
                              'readme': 'contrib/macosx/Readme.html',
                              'welcome': 'contrib/macosx/Welcome.html',
                              },
               },
      **extra)
