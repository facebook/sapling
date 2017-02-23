from distutils.version import LooseVersion
from distutils.core import setup, Extension
import distutils
from glob import glob

import os, sys

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
    cflags.extend(["-O0", "-g"])
else:
    cflags.append("-Werror")

def get_env_path_list(var_name, default=None):
    '''Get a path list from an environment variable.  The variable is parsed as
    a colon-separated list.'''
    value = os.environ.get(var_name)
    if not value:
        return default
    return value.split(os.path.pathsep)

include_dirs = get_env_path_list('INCLUDE_DIRS')
library_dirs = get_env_path_list('LIBRARY_DIRS')

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
    include_dirs = [
        '/usr/local/include',
        '/opt/local/include',
        '/opt/homebrew/include/',
    ]

def distutils_dir_name(dname):
    """Returns the name of a distutils build directory"""
    f = "{dirname}.{platform}-{version}"
    return f.format(dirname=dname,
                    platform=distutils.util.get_platform(),
                    version=sys.version[:3])

if library_dirs is None:
    library_dirs = [
        '/usr/local/lib',
        '/opt/local/lib',
        '/opt/homebrew/lib/',
        'build/' + distutils_dir_name('lib'),
    ]

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
        libraries = build_info.get('libraries')
        for lib in libraries:
            self.compiler.add_library(lib)
        self.compiler.create_static_lib(objects, lib_name,
                                        output_dir=self.build_clib,
                                        debug=self.debug)
distutils.command.build_clib.build_clib.build_libraries = build_libraries

# Static c libaries
libraries = [
    ("datapack", {
        "sources" : ["cdatapack/cdatapack.c"],
        "include_dirs" : ["clib"] + include_dirs,
        "libraries" : ["lz4", "crypto"],
        "extra_args" : [
            "-std=c99",
            "-Wall",
            "-Werror", "-Werror=strict-prototypes",
        ] + cflags,
    }),
]

hgext3rd = [
    p[:-3].replace('/', '.')
    for p in glob('hgext3rd/*.py')
    if p != 'hgext3rd/__init__.py'
]

availablepymodules = dict([(x[9:], x) for x in hgext3rd])
availablepymodules['statprof'] = 'statprof'

availablepackages = [
    'fastannotate',
    'fastmanifest',
    'infinitepush',
    'phabricator',
    'sqldirstate',
    'remotefilelog',
    'treemanifest',
    'linelog',
]

def distutils_dir_name(dname):
    """Returns the name of a distutils build directory"""
    f = "{dirname}.{platform}-{version}"
    return f.format(dirname=dname,
                    platform=distutils.util.get_platform(),
                    version=sys.version[:3])

if os.name == 'nt':
    # The modules that successfully compile on Windows
    availableextmodules = {}
else:
    availableextmodules = {
        'cstore' : [
            Extension('cstore',
                sources=[
                    'cstore/datapackstore.cpp',
                    'cstore/py-cstore.cpp',
                    'cstore/uniondatapackstore.cpp',
                    'ctreemanifest/manifest.cpp',
                    'ctreemanifest/manifest_entry.cpp',
                    'ctreemanifest/manifest_fetcher.cpp',
                    'ctreemanifest/pythonutil.cpp',
                    'ctreemanifest/treemanifest.cpp',
                ],
                include_dirs=[
                    'cdatapack',
                    'clib'
                    'cstore',
                    'ctreemanifest',
                ] + include_dirs,
                library_dirs=[
                    'build/' + distutils_dir_name('lib'),
                ] + library_dirs,
                libraries=[
                    'crypto',
                    'datapack',
                    'lz4',
                ],
                extra_compile_args=[
                    "-std=c++0x",
                    "-Wall",
                ] + cflags,
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
                libraries=['crypto',
                ],
                extra_compile_args=[
                    "-std=c99",
                    "-Wall",
                    "-Werror=strict-prototypes",
                ] + cflags,
            ),
        ],
        'linelog' : [
            Extension('linelog',
                sources=['linelog/pyext/linelog.pyx'],
                extra_compile_args=[
                    '-std=c99',
                    '-Wall', '-Wextra', '-Wconversion', '-pedantic',
                ] + cflags,
            ),
        ],
    }

COMPONENTS = sorted(availablepackages + availableextmodules.keys() +
                    availablepymodules.keys())

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

if os.name == 'nt':
    # The modules that successfully compile on Windows
    cythonmodules = []
else:
    cythonmodules = [
        'linelog',
    ]
for cythonmodule in cythonmodules:
    if cythonmodule in components:
        import Cython
        if LooseVersion(Cython.__version__) < LooseVersion('0.22'):
            raise RuntimeError('Cython >= 0.22 is required')

        from Cython.Build import cythonize
        module = availableextmodules[cythonmodule]
        availableextmodules[cythonmodule] = cythonize(module)

packages = []
for package in availablepackages:
    if package in components:
        packages.append(package)

ext_modules = []
for ext_module in availableextmodules:
    if ext_module in components:
        ext_modules.extend(availableextmodules[ext_module])

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
    if module in components:
        py_modules.append(availablepymodules[module])

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
    libraries = libraries,
)
