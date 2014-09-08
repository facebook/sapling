from distutils.core import setup, Extension

setup(
    name='fb-hg-experimental',
    version='0.1.0',
    author='Durham Goode',
    maintainer='Durham Goode',
    maintainer_email='durham@fb.com',
    url='',
    description='Experimental Mercurial extensions from Facebook',
    long_description="",
    keywords='fb hg mercurial',
    license='',
    py_modules=[
        'backups',
        'fbamend',
        'githelp',
        'smartlog',
    ]
)
