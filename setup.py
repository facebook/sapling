from distutils.version import LooseVersion
from distutils.cmd import Command
from distutils.core import setup, Extension
import distutils
import fnmatch
from glob import glob

import os, shutil, sys

iswindows = os.name == 'nt'
WERROR = "/WX" if iswindows else "-Werror"
WSTRICTPROTOTYPES = None if iswindows else "-Werror=strict-prototypes"
WALL = "/Wall" if iswindows else "-Wall"
STDC99 = "" if iswindows else "-std=c99"
STDCPP0X = "" if iswindows else "-std=c++0x"
WEXTRA = "" if iswindows else "-Wextra"
WCONVERSION = "" if iswindows else "-Wconversion"
PEDANTIC = "" if iswindows else "-pedantic"
NOOPTIMIZATION = "/Od" if iswindows else "-O0"
OPTIMIZATION = "" if iswindows else "-O2"
PRODUCEDEBUGSYMBOLS = "/DEBUG:FULL" if iswindows else "-g"

# whether to use Cython to recompile .pyx to .c/.cpp at build time.
# if False, fallback to .c/.cpp in the repo and .pyx files are ignored.
# if True, re-compile .c/.cpp from .pyx files, require cython at build time.
if 'USECYTHON' in os.environ:
    USECYTHON = int(os.environ['USECYTHON'])
else:
    try:
        import Cython
    except ImportError:
        USECYTHON = False
    else:
        USECYTHON = (LooseVersion(Cython.__version__) >= LooseVersion('0.22'))

# --component allows the caller to specify what components they want. We can't
# use argparse like normal, because setup() at the bottom has it's own argument
# logic.
components = []
args = []
skip = False
for i, arg in enumerate(sys.argv):
    if skip:
        skip = False
        continue

    if arg == '--component' and len(sys.argv) > i + 1:
        components.extend(sys.argv[i + 1].split(','))
        skip = True
    else:
        args.append(arg)

sys.argv = args

cflags = []

# if this is set, compile all C extensions with -O0 -g for easy debugging.  note
# that this is not manifested in any way in the Makefile dependencies.
# therefore, if you already have build products, they won't be rebuilt!
if os.getenv('FB_HGEXT_CDEBUG') is not None:
    cflags.extend([NOOPTIMIZATION, PRODUCEDEBUGSYMBOLS])
else:
    cflags.append(WERROR)

def get_env_path_list(var_name, default=None):
    '''Get a path list from an environment variable.  The variable is parsed as
    a colon-separated list.'''
    value = os.environ.get(var_name)
    if not value:
        return default
    return value.split(os.path.pathsep)

include_dirs = get_env_path_list('INCLUDE_DIRS')
library_dirs = get_env_path_list('LIBRARY_DIRS')

def filter_existing_dirs(dirs):
    '''Filters the given list and keeps only existing directory names.'''
    return [d for d in dirs if os.path.isdir(d)]

# Historical default values.
# We should perhaps clean these up in the future after verifying that it
# doesn't break the build on any platforms.
#
# The /usr/local/* directories shouldn't actually be needed--the compiler
# should already use these directories when appropriate (e.g., if we are
# using the standard system compiler that has them in its default paths).
#
# The /opt/local paths may be necessary on Darwin builds.
if include_dirs is None:
    if iswindows:
        include_dirs = []
    else:
        include_dirs = filter_existing_dirs([
            '/usr/local/include',
            '/opt/local/include',
            '/opt/homebrew/include/',
        ])

def distutils_dir_name(dname):
    """Returns the name of a distutils build directory"""
    f = "{dirname}.{platform}-{version}"
    return f.format(dirname=dname,
                    platform=distutils.util.get_platform(),
                    version=sys.version[:3])

if library_dirs is None:
    if iswindows:
        library_dirs = []
    else:
        library_dirs = filter_existing_dirs([
            '/usr/local/lib',
            '/opt/local/lib',
            '/opt/homebrew/lib/',
        ])
    library_dirs.append('build/' + distutils_dir_name('lib'))

# Override the default c static library building code in distutils since it
# doesn't pass enough args, like libraries and extra args.
import distutils.command.build_clib
from distutils.errors import DistutilsSetupError
def build_libraries(self, libraries):
    for (lib_name, build_info) in libraries:
        sources = build_info.get('sources')
        if sources is None or not isinstance(sources, (list, tuple)):
            raise DistutilsSetupError(
                   "in 'libraries' option (library '%s'), " +
                   "'sources' must be present and must be " +
                   "a list of source filenames") % lib_name
        sources = list(sources)

        # First, compile the source code to object files in the library
        # directory.  (This should probably change to putting object
        # files in a temporary build directory.)
        macros = build_info.get('macros')
        include_dirs = build_info.get('include_dirs')
        extra_args = build_info.get('extra_args')
        objects = self.compiler.compile(sources,
                                        output_dir=self.build_temp,
                                        macros=macros,
                                        include_dirs=include_dirs,
                                        debug=self.debug,
                                        extra_postargs=extra_args)

        # Now "link" the object files together into a static library.
        # (On Unix at least, this isn't really linking -- it just
        # builds an archive.  Whatever.)
        libraries = build_info.get('libraries', [])
        for lib in libraries:
            self.compiler.add_library(lib)
        self.compiler.create_static_lib(objects, lib_name,
                                        output_dir=self.build_clib,
                                        debug=self.debug)
distutils.command.build_clib.build_clib.build_libraries = build_libraries

# Static c libaries
if iswindows:
    availablelibraries = {}
else:
    availablelibraries = {
        'datapack': {
            "sources" : ["cdatapack/cdatapack.c"],
            "include_dirs" : ["clib"] + include_dirs,
            "libraries" : ["lz4", "sha1"],
            "extra_args" : filter(None,
                [STDC99, WALL, WERROR, WSTRICTPROTOTYPES] + cflags),
        },
        'mpatch': {
            'sources': ['cstore/mpatch.c']
        },
        "sha1": {
            "sources" : ["clib/sha1/sha1.c", "clib/sha1/ubc_check.c"],
            "include_dirs" : ["clib/sha1"] + include_dirs,
            "extra_args" : filter(None,
                [STDC99, WALL, WERROR, WSTRICTPROTOTYPES] + cflags),
        },
    }

# modules that are single files in hgext3rd
hgext3rd = [
    p[:-3].replace('/', '.')
    for p in glob('hgext3rd/*.py')
    if p != 'hgext3rd/__init__.py'
]

# packages that are directories in hgext3rd
hgext3rdpkgs = [
    p[:-12].replace('/', '.')
    for p in glob('hgext3rd/*/__init__.py')
]

availablepymodules = hgext3rd

availablepackages = hgext3rdpkgs + [
    'infinitepush',
    'phabricator',
    'sqldirstate',
    'remotefilelog',
]

if iswindows:
    availablepackages += [
        'linelog',
    ]
else:
    availablepackages += [
        'fastmanifest',
        'treemanifest',
        'linelog',
    ]

def distutils_dir_name(dname):
    """Returns the name of a distutils build directory"""
    f = "{dirname}.{platform}-{version}"
    return f.format(dirname=dname,
                    platform=distutils.util.get_platform(),
                    version=sys.version[:3])

if iswindows:
    # The modules that successfully compile on Windows
    availableextmodules = {
        'linelog' : [
            Extension('linelog',
                sources=['linelog/pyext/linelog.pyx'],
                extra_compile_args=filter(None, [
                    STDC99, WALL, WEXTRA, WCONVERSION, PEDANTIC,
                ]),
            ),
        ],
    }
else:
    availableextmodules = {
        'cstore' : [
            Extension('cstore',
                sources=[
                    'cstore/datapackstore.cpp',
                    'cstore/py-cstore.cpp',
                    'cstore/pythonutil.cpp',
                    'cstore/uniondatapackstore.cpp',
                    'ctreemanifest/manifest.cpp',
                    'ctreemanifest/manifest_entry.cpp',
                    'ctreemanifest/manifest_fetcher.cpp',
                    'ctreemanifest/treemanifest.cpp',
                ],
                include_dirs=[
                    'ctreemanifest',
                    'cdatapack',
                    'clib',
                    'cstore',
                ] + include_dirs,
                library_dirs=[
                    'build/' + distutils_dir_name('lib'),
                ] + library_dirs,
                libraries=[
                    'datapack',
                    'lz4',
                    'mpatch',
                    'sha1',
                ],
                extra_compile_args=filter(None, [STDCPP0X, WALL] + cflags),
            ),
        ],
        'cfastmanifest' : [
            Extension('cfastmanifest',
                sources=['cfastmanifest.c',
                         'cfastmanifest/bsearch.c',
                         'clib/buffer.c',
                         'cfastmanifest/checksum.c',
                         'cfastmanifest/node.c',
                         'cfastmanifest/tree.c',
                         'cfastmanifest/tree_arena.c',
                         'cfastmanifest/tree_convert.c',
                         'cfastmanifest/tree_copy.c',
                         'cfastmanifest/tree_diff.c',
                         'cfastmanifest/tree_disk.c',
                         'cfastmanifest/tree_iterator.c',
                         'cfastmanifest/tree_path.c',
                ],
                include_dirs=[
                    'cfastmanifest',
                    'clib',
                ] + include_dirs,
                library_dirs=library_dirs,
                libraries=['sha1'],
                extra_compile_args=filter(None, [
                    STDC99,
                    WALL,
                    WSTRICTPROTOTYPES,
                ] + cflags),
            ),
        ],
        'linelog' : [
            Extension('linelog',
                sources=['linelog/pyext/linelog.pyx'],
                extra_compile_args=filter(None, [
                    STDC99, WALL, WEXTRA, WCONVERSION, PEDANTIC,
                ]),
            ),
        ],
        'patchrmdir': [
            Extension('hgext3rd.patchrmdir',
                sources=['hgext3rd/patchrmdir.pyx'],
                extra_compile_args=filter(None, [
                    STDC99, WALL, WEXTRA, WCONVERSION, PEDANTIC,
                ]),
            ),
        ],
        'traceprof': [
            Extension('hgext3rd.traceprof',
                sources=['hgext3rd/traceprof.pyx'],
                include_dirs=['hgext3rd/'],
                extra_compile_args=filter(None, [
                    OPTIMIZATION, STDCPP0X, WALL, WEXTRA, WCONVERSION, PEDANTIC,
                    PRODUCEDEBUGSYMBOLS
                ]),
            ),
        ]
    }

allnames = availablepackages + availableextmodules.keys() + availablepymodules
COMPONENTS = sorted(name.split('.')[-1] for name in allnames)

if not components:
    components = COMPONENTS

dependencies = {
    'absorb' : ['linelog'],
    'cstore' : ['ctreemanifest', 'cdatapack'],
    'fastannotate' : ['linelog'],
    'infinitepush' : ['extutil'],
    'remotefilelog' : ['cstore', 'extutil'],
    'treemanifest' : ['cstore'],
}

processdep = True
while processdep:
    processdep = False
    for name, deps in dependencies.iteritems():
        if name in components:
            for dep in deps:
                if dep not in components:
                    components.append(dep)
                    processdep = True

if iswindows:
    # The modules that successfully compile on Windows
    cythonmodules = ['linelog']
else:
    cythonmodules = [
        'linelog',
        'patchrmdir',
        'traceprof',
    ]

if USECYTHON:
    # see http://cython.readthedocs.io/en/latest/src/reference/compilation.html
    compileroptions = {
        'unraisable_tracebacks': False,
        'c_string_type': 'bytes',
    }
    for cythonmodule in cythonmodules:
        if cythonmodule in components:
            module = availableextmodules[cythonmodule]
            try:
                from Cython.Build import cythonize
                availableextmodules[cythonmodule] = cythonize(
                    module,
                    compiler_directives=compileroptions,
                )
            except Exception:
                # ImportError or Cython.Compiler.Errors.CompileError
                sys.stderr.write(
                    '+------------------------------------------------+\n'
                    '| Failed to run cythonize.                       |\n'
                    '| Make sure you have Cython >= 0.21.1 installed. |\n'
                    '+------------------------------------------------+\n')
                raise SystemExit(255)
else:
    # use prebuilt files under prebuilt/cython
    # change module sources from .pyx to .c or .cpp files
    for cythonmodule in cythonmodules:
        for m in availableextmodules[cythonmodule]:
            sources = m.sources
            iscpp = 'c++' in open(sources[0]).readline()
            ext = iscpp and '.cpp' or '.c'
            dstpaths = []
            for src in sources:
                dst = src.replace('.pyx', ext)
                dstpaths.append(dst)
                shutil.copy(os.path.join('prebuilt', 'cython',
                                         os.path.basename(dst)), dst)
            m.sources = dstpaths

packages = []
for package in availablepackages:
    if package.split('.')[-1] in components:
        packages.append(package)

librarynames = set()
ext_modules = []
for ext_module in availableextmodules:
    if ext_module in components:
        modules = availableextmodules[ext_module]
        ext_modules.extend(modules)
        librarynames.update(l for m in modules for l in m.libraries)

libraries = [(n, availablelibraries[n])
             for n in librarynames if n in availablelibraries]

# Dependencies between our native libraries means we need to build in order
ext_order = {
    'libdatapack' : 0,
    'cstore' : 3,
}
ext_modules = sorted(ext_modules, key=lambda k: ext_order.get(k.name, 999))

requires = []
requireslz4 = ['remotefilelog', 'cdatapack']
if any(c for c in components if c in requireslz4):
    requires.append('lz4')

py_modules = []
for module in availablepymodules:
    if module.split('.')[-1] in components:
        py_modules.append(module)

# Extra clean command cleaning up non-Python extensions
class CleanExtCommand(Command):
    description = 'remove extra build files'
    user_options = []

    def initialize_options(self):
        pass

    def finalize_options(self):
        pass

    def run(self):
        root = os.path.dirname(os.path.abspath(__file__))
        os.chdir(root)

        # removed counter (ext: count)
        removed = {}

        def removepath(path):
            try:
                os.unlink(path)
            except OSError: # ENOENT
                pass
            else:
                ext = path.split('.')[-1]
                removed.setdefault(ext, 0)
                removed[ext] += 1

        # remove *.o not belonging to Python extensions, and .py[cdo], .so files
        for pat in ['*.o', '*.py[cdo]', '*.so']:
            for path in self._rglob(pat):
                removepath(path)

        # remove .c generated from Cython .pyx
        for path in self._rglob('*.pyx'):
            cpath = '%s.c' % path[:-4]
            removepath(cpath)
            cpppath = cpath + 'pp'
            removepath(cpppath)

        # print short summary
        if removed:
            summary = 'removed %s files' % (
                ', '.join('%s .%s' % (count, ext)
                          for ext, count in sorted(removed.iteritems())))
            self.announce(summary, level=distutils.log.INFO)

    def _rglob(self, patten):
        # recursive glob
        for dirname, dirs, files in os.walk('.'):
            for name in fnmatch.filter(files, patten):
                yield os.path.join(dirname, name)

setup(
    name='fbhgext',
    version='1.0',
    author='Facebook Source Control Team',
    maintainer='Facebook Source Control Team',
    maintainer_email='sourcecontrol-dev@fb.com',
    url='https://bitbucket.org/facebook/hg-experimental/',
    description='Facebook mercurial extensions',
    long_description="",
    keywords='facebook fb hg mercurial shallow remote filelog',
    license='GPLv2+',
    packages=packages,
    install_requires=requires,
    py_modules=py_modules,
    ext_modules = ext_modules,
    libraries=libraries,
    cmdclass={
        'clean_ext': CleanExtCommand,
    }
)
