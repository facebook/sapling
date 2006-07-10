#!/usr/bin/env python
# Encoding: iso-8859-1
# vim: tw=80 ts=4 sw=4 noet
# -----------------------------------------------------------------------------
# Project   : Basic Darcs to Mercurial conversion script
# -----------------------------------------------------------------------------
# Authors   : Sebastien Pierre                           <sebastien@xprima.com>
#             TK Soh                                      <teekaysoh@gmail.com>
# -----------------------------------------------------------------------------
# Creation  : 24-May-2006
# Last mod  : 01-Jun-2006
# -----------------------------------------------------------------------------

import os, sys
import tempfile
import xml.dom.minidom as xml_dom
from time import strptime, mktime

DARCS_REPO = None
HG_REPO    = None

USAGE = """\
%s DARCSREPO HGREPO

    Converts the given Darcs repository to a new Mercurial repository. The given
    HGREPO must not exist, as it will be created and filled up (this will avoid
    overwriting valuable data.

""" % (os.path.basename(sys.argv[0]))

# ------------------------------------------------------------------------------
#
# Utilities
#
# ------------------------------------------------------------------------------

def cmd(text, path=None, silent=False):
	"""Executes a command, in the given directory (if any), and returns the
	command result as a string."""
	cwd = None
	if path:
		path = os.path.abspath(path)
		cwd  = os.getcwd()
		os.chdir(path)
	if not silent: print "> ", text
	res = os.popen(text).read()
	if path:
		os.chdir(cwd)
	return res

def writefile(path, data):
	"""Writes the given data into the given file."""
	f = file(path, "w") ; f.write(data)  ; f.close()

def error( *args ):
	sys.stderr.write("ERROR:")
	for a in args: sys.stderr.write(str(a))
	sys.stderr.write("\n")
	sys.exit(-1)

# ------------------------------------------------------------------------------
#
# Darcs interface
#
# ------------------------------------------------------------------------------

def darcs_changes(darcsRepo):
	"""Gets the changes list from the given darcs repository. This returns the
	chronological list of changes as (change name, change summary)."""
	changes    = cmd("darcs changes --reverse --xml-output", darcsRepo)
	doc        = xml_dom.parseString(changes)
	for patch_node in doc.childNodes[0].childNodes:
		name = filter(lambda n:n.nodeName == "name", patch_node.childNodes)
		comm = filter(lambda n:n.nodeName == "comment", patch_node.childNodes)
		if not name:continue
		else: name = name[0].childNodes[0].data
		if not comm: comm = ""
		else: comm = comm[0].childNodes[0].data
		author = patch_node.getAttribute("author")
		date = patch_node.getAttribute("date")
        hash = patch_node.getAttribute("hash")
        yield hash, author, date, name, comm

def darcs_pull(hg_repo, darcs_repo, change):
	cmd("darcs pull '%s' --all --patches='%s'" % (darcs_repo, change), hg_repo)

# ------------------------------------------------------------------------------
#
# Mercurial interface
#
# ------------------------------------------------------------------------------

def hg_commit( hg_repo, text, author, date ):
	fd, tmpfile = tempfile.mkstemp(prefix="darcs2hg_")
	writefile(tmpfile, text)
	cmd("hg add -X _darcs", hg_repo)
	cmd("hg remove -X _darcs --after", hg_repo)
	cmd("hg commit -l %s -u '%s' -d '%s 0'"  % (tmpfile, author, date), hg_repo)
	os.unlink(tmpfile)

# ------------------------------------------------------------------------------
#
# Main
#
# ------------------------------------------------------------------------------

if __name__ == "__main__":
	args = sys.argv[1:]
	# We parse the arguments
	if len(args)   == 2:
		darcs_repo = os.path.abspath(args[0])
		hg_repo    = os.path.abspath(args[1])
	else:
		print USAGE
		sys.exit(-1)
	# Initializes the target repo
	if not os.path.isdir(darcs_repo + "/_darcs"):
		print "No darcs directory found at: " + darcs_repo
		sys.exit(-1)
	if not os.path.isdir(hg_repo):
		os.mkdir(hg_repo)
	else:
		print "Given HG repository must not exist. It will be created"
		sys.exit(-1)
	cmd("hg init '%s'" % (hg_repo))
	cmd("darcs initialize", hg_repo)
	# Get the changes from the Darcs repository
	for hash, author, date, summary, description in darcs_changes(darcs_repo):
		text = summary + "\n" + description
		darcs_pull(hg_repo, darcs_repo, hash)
		epoch = int(mktime(strptime(date, '%Y%m%d%H%M%S')))
		hg_commit(hg_repo, text, author, epoch)

# EOF
