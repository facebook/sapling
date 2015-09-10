from distutils.core import setup, Extension

setup(
    name='remotefilelog',
    version='0.2',
    author='Durham Goode',
    maintainer='Durham Goode',
    maintainer_email='durham@fb.com',
    url='https://bitbucket.org/facebook/remotefilelog',
    description='Remote filelog extension for Mercurial',
    long_description="""
This extension adds support for remote filelogs in Mercurial where all the file history is stored remotely.
    """.strip(),
    keywords='hg shallow mercurial remote filelog',
    license='GPLv2+',
    packages=['remotefilelog'],
    install_requires=['lz4'],
    ext_modules = []
)
