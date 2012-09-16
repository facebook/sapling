try:
    from setuptools import setup
except:
    from distutils.core import setup

try:
    from collections import OrderedDict
    extra_req = []
except ImportError:
    extra_req = ['ordereddict>=1.1']

setup(
    name='hg-git',
    version='0.4.0',
    author='Scott Chacon',
    maintainer='Augie Fackler',
    maintainer_email='durin42@gmail.com',
    url='http://hg-git.github.com/',
    description='push and pull from a Git server using Mercurial',
    long_description="""
This extension lets you communicate (push and pull) with a Git server.
This way you can use Git hosting for your project or collaborate with a
project that is in Git.  A bridger of worlds, this plugin be.
    """.strip(),
    keywords='hg git mercurial',
    license='GPLv2',
    packages=['hggit'],
    package_data={ 'hggit': ['help/git.rst'] },
    install_requires=['dulwich>=0.8.6'] + extra_req,
)
