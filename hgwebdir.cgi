#!/usr/bin/env python
#
# An example CGI script to export multiple hgweb repos, edit as necessary

# adjust python path if not a system-wide install:
#import sys
#sys.path.insert(0, "/path/to/python/lib")

# enable importing on demand to reduce startup time
from mercurial import demandimport; demandimport.enable()

# Uncomment to send python tracebacks to the browser if an error occurs:
#import cgitb
#cgitb.enable()

# If you'd like to serve pages with UTF-8 instead of your default
# locale charset, you can do so by uncommenting the following lines.
# Note that this will cause your .hgrc files to be interpreted in
# UTF-8 and all your repo files to be displayed using UTF-8.
#
#import os
#os.environ["HGENCODING"] = "UTF-8"

from mercurial.hgweb.hgwebdir_mod import hgwebdir
import mercurial.hgweb.wsgicgi as wsgicgi

# The config file looks like this.  You can have paths to individual
# repos, collections of repos in a directory tree, or both.
#
# [paths]
# virtual/path1 = /real/path1
# virtual/path2 = /real/path2
# virtual/root = /real/root/*
# / = /real/root2/*
# virtual/root2 = /real/root2/**
#
# [collections]
# /prefix/to/strip/off = /root/of/tree/full/of/repos
#
# paths example:
#
# * First two lines mount one repository into one virtual path, like
# '/real/path1' into 'virtual/path1'.
#
# * The third entry mounts every mercurial repository found in '/real/root'
# in 'virtual/root'. This format is preferred over the [collections] one,
# since using absolute paths as configuration keys is not supported on every
# platform (especially on Windows).
#
# * The fourth entry is a special case mounting all repositories in
# /'real/root2' in the root of the virtual directory.
#
# * The fifth entry recursively finds all repositories under the real root,
# and mounts them using their relative path (to given real root) under the
# virtual root.
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
wsgicgi.launch(application)
