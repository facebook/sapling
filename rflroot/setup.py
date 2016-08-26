from distutils.core import setup, Extension

setup(
    name='remotefilelog',
    version='0.2',
    author='Durham Goode',
    maintainer='Durham Goode',
    maintainer_email='durham@fb.com',
    url='https://bitbucket.org/facebook/remotefilelog',
    description='Remote filelog extension for Mercurial',
    long_description="This extension adds support for remote filelogs in "
        "Mercurial where all the file history is stored remotely.".strip(),
    keywords='hg shallow mercurial remote filelog',
    license='GPLv2+',
    packages=['remotefilelog'],
    install_requires=['lz4'],
    ext_modules = [
        Extension('cdatapack',
                  sources=[
                      'remotefilelog/cdatapack/py-cdatapack.c',
                      'remotefilelog/cdatapack/cdatapack.c',
                  ],
                  include_dirs=[
                      'remotefilelog/cdatapack',
                      '/usr/local/include',
                      '/opt/local/include',
                  ],
                  library_dirs=[
                      '/usr/local/lib',
                      '/opt/local/lib',
                  ],
                  libraries=[
                      'crypto',
                      'lz4',
                  ],
                  extra_compile_args=[
                      "-std=c99",
                      "-Wall",
                      "-Werror", "-Werror=strict-prototypes"],
        ),
        Extension('ctreemanifest',
                  sources=[
                      'remotefilelog/ctreemanifest/py-treemanifest.cpp',
                      'remotefilelog/ctreemanifest/manifest.cpp',
                      'remotefilelog/ctreemanifest/manifest_entry.cpp',
                      'remotefilelog/ctreemanifest/manifest_fetcher.cpp',
                      'remotefilelog/ctreemanifest/pythonutil.cpp',
                      'remotefilelog/ctreemanifest/treemanifest.cpp',
                  ],
                  include_dirs=[
                      'remotefilelog/ctreemanifest',
                  ],
                  library_dirs=[
                      '/usr/local/lib',
                      '/opt/local/lib',
                  ],
                  libraries=[
                  ],
                  extra_compile_args=[
                      "-Wall",
                      "-Werror", "-Werror=strict-prototypes"],
        )
    ],
)
