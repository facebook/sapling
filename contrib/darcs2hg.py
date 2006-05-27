#!/usr/bin/env python
# Encoding: iso-8859-1
# vim: tw=80 ts=4 sw=4 noet
# -----------------------------------------------------------------------------
# Project   : Basic Darcs to Mercurial conversion script
# -----------------------------------------------------------------------------
# Author    : Sebastien Pierre <sebastien@xprima.com>
# Creation  : 24-May-2006
# Last mod  : 26-May-2006
# History   :
#             26-May-2006 - Updated
#             24-May-2006 - First implementation
# -----------------------------------------------------------------------------

import os, sys
import xml.dom.minidom as xml_dom

DARCS_REPO = None
HG_REPO    = None

USAGE = """\
%s DARCSREPO HGREPO

    Converts the given Darcs repository to a new Mercurial repository. The given
    HGREPO must not exist, as it will be created and filled up (this will avoid
    overwriting valuable data.

""" % (os.path.basename(__file__))

# ------------------------------------------------------------------------------
#
# Utilities
#
# ------------------------------------------------------------------------------

def cmd(text, path=None):
	"""Executes a command, in the given directory (if any), and returns the
	command result as a string."""
	cwd = None
	if path:
		path = os.path.abspath(path)
		cwd  = os.getcwd()
		os.chdir(path)
	print text
	res = os.popen(text).read()
	if path:
		os.chdir(cwd)
	return res

def writefile(path, data):
	"""Writes the given data into the given file."""
	f = file(path, "w") ; f.write(data)  ; f.close()

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
	res        = []
	for patch_node in doc.childNodes[0].childNodes:
		name = filter(lambda n:n.nodeName == "name", patch_node.childNodes)
		comm = filter(lambda n:n.nodeName == "comment", patch_node.childNodes)
		if not name:continue
		else: name = name[0].childNodes[0].data
		if not comm: comm = ""
		else: comm = comm[0].childNodes[0].data
		res.append([name, comm])
	return res

def darcs_pull(hg_repo, darcs_repo, change):
	cmd("darcs pull '%s' --all --patches='%s'" % (darcs_repo, change), hg_repo)

# ------------------------------------------------------------------------------
#
# Mercurial interface
#
# ------------------------------------------------------------------------------

def hg_commit( hg_repo, text ):
	writefile("/tmp/msg", text)
	cmd("hg add -X _darcs *", hg_repo)
	cmd("hg commit -l /tmp/msg", hg_repo)
	os.unlink("/tmp/msg")

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
		print "No darcs directory found at: " + darc_repo
		sys.exit(-1)
	if not os.path.isdir(hg_repo):
		os.mkdir(hg_repo)
	else:
		print "Given HG repository must not exist. It will be created"
		sys.exit(-1)
	cmd("hg init '%s'" % (hg_repo))
	cmd("darcs initialize", hg_repo)
	# Get the changes from the Darcs repository
	for summary, description in darcs_changes(darcs_repo):
		text = summary + "\n" + description
		darcs_pull(hg_repo, darcs_repo, summary)
		hg_commit(hg_repo, text)

# EOF

