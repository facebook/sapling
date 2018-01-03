from distutils.core import setup, Extension

setup(
    name='lz4revlog',
    version='1.0.2',
    author='Siddharth Agarwal',
    maintainer='Siddharth Agarwal',
    maintainer_email='sid0@fb.com',
    url='https://bitbucket.org/facebook/lz4revlog',
    description='lz4revlog: Mercurial revlogs compressed using lz4',
    long_description="",
    keywords='hg mercurial lz4',
    license='GNU GPLv2 or any later version',
    py_modules=['lz4revlog']
)
