# An example WSGI (use with mod_wsgi) script to export multiple hgweb repos

# adjust python path if not a system-wide install:
#import sys
#sys.path.insert(0, "/path/to/python/lib")

# enable demandloading to reduce startup time
from mercurial import demandimport; demandimport.enable()
from mercurial.hgweb.hgwebdir_mod import hgwebdir

# If you'd like to serve pages with UTF-8 instead of your default
# locale charset, you can do so by uncommenting the following lines.
# Note that this will cause your .hgrc files to be interpreted in
# UTF-8 and all your repo files to be displayed using UTF-8.
#
#import os
#os.environ["HGENCODING"] = "UTF-8"

# The config file looks like this.  You can have paths to individual
# repos, collections of repos in a directory tree, or both.
#
# [paths]
# virtual/path1 = /real/path1
# virtual/path2 = /real/path2
# virtual/root = /real/root/*
# / = /real/root2/*
#
# paths example:
#
# * First two lines mount one repository into one virtual path, like
# '/real/path1' into 'virtual/path1'.
#
# * The third entry tells every mercurial repository found in
# '/real/root', recursively, should be mounted in 'virtual/root'. This
# format is preferred over the [collections] one, using absolute paths
# as configuration keys is not supported on every platform (including
# Windows).
#
# * The last entry is a special case mounting all repositories in
# '/real/root2' in the root of the virtual directory.
#
# collections example: say directory tree /foo contains repos /foo/bar,
# /foo/quux/baz.  Give this config section:
#   [collections]
#   /foo = /foo
# Then repos will list as bar and quux/baz.
#
# Alternatively you can pass a list of ('virtual/path', '/real/path') tuples
# or use a dictionary with entries like 'virtual/path': '/real/path'

application = hgwebdir('hgweb.config')
