#!/usr/bin/env python
# Encoding: iso-8859-1
# vim: tw=80 ts=4 sw=4 noet
# -----------------------------------------------------------------------------
# Project   : Basic Darcs to Mercurial conversion script
#
# *** DEPRECATED. Use the convert extension instead. This script will
# *** be removed soon.
#
# -----------------------------------------------------------------------------
# Authors   : Sebastien Pierre                           <sebastien@xprima.com>
#             TK Soh                                      <teekaysoh@gmail.com>
# -----------------------------------------------------------------------------
# Creation  : 24-May-2006
# -----------------------------------------------------------------------------

import os, sys
import tempfile
import xml.dom.minidom as xml_dom
from time import strptime, mktime
import re

DARCS_REPO = None
HG_REPO    = None

USAGE = """\
%s DARCSREPO HGREPO [SKIP]

    Converts the given Darcs repository to a new Mercurial repository. The given
    HGREPO must not exist, as it will be created and filled up (this will avoid
    overwriting valuable data.

    In case an error occurs within the process, you can resume the process by
    giving the last successfuly applied change number.
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
	sys.stderr.write("ERROR: ")
	for a in args: sys.stderr.write(str(a))
	sys.stderr.write("\n")
	sys.stderr.write("You can make manual fixes if necessary and then resume by"
	" giving the last changeset number")
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
		name = filter(lambda n: n.nodeName == "name", patch_node.childNodes)
		comm = filter(lambda n: n.nodeName == "comment", patch_node.childNodes)
		if not name:continue
		else: name = name[0].childNodes[0].data
		if not comm: comm = ""
		else: comm = comm[0].childNodes[0].data
		author = patch_node.getAttribute("author")
		date   = patch_node.getAttribute("date")
		chash  = os.path.splitext(patch_node.getAttribute("hash"))[0]
		yield author, date, name, chash, comm

def darcs_tip(darcs_repo):
	changes = cmd("darcs changes",darcs_repo,silent=True)
	changes = filter(lambda l: l.strip().startswith("* "), changes.split("\n"))
	return len(changes)

def darcs_pull(hg_repo, darcs_repo, chash):
	old_tip = darcs_tip(darcs_repo)
	res     = cmd("darcs pull \"%s\" --all --match=\"hash %s\"" % (darcs_repo, chash), hg_repo)
	if re.search('^We have conflicts in the following files:$', res, re.MULTILINE):
		print "Trying to revert files to work around conflict..."
		rev_res = cmd ("darcs revert --all", hg_repo)
		print rev_res
	print res
	new_tip = darcs_tip(darcs_repo)
	if not new_tip != old_tip + 1:
		error("Darcs pull did not work as expected: " + res)

def darcs_changes_summary(darcs_repo, chash):
	"""Gets the changes from the darcs summary. This returns the chronological
	list of changes as (change_type, args). Eg. ('add_file', 'foo.txt') or
	('move', ['foo.txt','bar.txt'])."""
	change = cmd("darcs changes --summary --xml-output --match=\"hash %s\"" % (chash), darcs_repo)
	doc = xml_dom.parseString(change)
	for patch_node in doc.childNodes[0].childNodes:
		summary_nodes = filter(lambda n: n.nodeName == "summary" and n.nodeType == n.ELEMENT_NODE, patch_node.childNodes)
		for summary_node in summary_nodes:
			change_nodes = filter(lambda n: n.nodeType == n.ELEMENT_NODE, summary_node.childNodes)
			if len(change_nodes) == 0:
				name = filter(lambda n: n.nodeName == "name", patch_node.childNodes)
				if not name:
					error("Darcs patch has an empty summary node and no name: " + patch_node.toxml())
				name = name[0].childNodes[0].data.strip()
				(tag, sub_count) = re.subn('^TAG ', '', name, 1)
				if sub_count != 1:
					error("Darcs patch has an empty summary node but doesn't look like a tag: " + patch_node.toxml());
			for change_node in change_nodes:
				change = change_node.nodeName
				if change == 'modify_file':
					yield change, change_node.childNodes[0].data.strip()
				elif change == 'add_file':
					yield change, change_node.childNodes[0].data.strip()
				elif change == 'remove_file':
					yield change, change_node.childNodes[0].data.strip()
				elif change == 'add_directory':
					yield change, change_node.childNodes[0].data.strip()
				elif change == 'remove_directory':
					yield change, change_node.childNodes[0].data.strip()
				elif change == 'move':
					yield change, (change_node.getAttribute('from'), change_node.getAttribute('to'))
				else:
					error('Problem parsing summary xml: Unexpected element: ' + change_node.toxml())

# ------------------------------------------------------------------------------
#
# Mercurial interface
#
# ------------------------------------------------------------------------------

def hg_commit( hg_repo, text, author, date ):
	fd, tmpfile = tempfile.mkstemp(prefix="darcs2hg_")
	writefile(tmpfile, text)
	old_tip = hg_tip(hg_repo)
	cmd("hg add -X _darcs", hg_repo)
	cmd("hg remove -X _darcs --after", hg_repo)
	res = cmd("hg commit -l %s -u \"%s\" -d \"%s 0\""  % (tmpfile, author, date), hg_repo)
	os.close(fd)
	os.unlink(tmpfile)
	new_tip = hg_tip(hg_repo)
	if not new_tip == old_tip + 1:
		# Sometimes we may have empty commits, we simply skip them
		if res.strip().lower().find("nothing changed") != -1:
			pass
		else:
			error("Mercurial commit did not work as expected: " + res)

def hg_tip( hg_repo ):
	"""Returns the latest local revision number in the given repository."""
	tip = cmd("hg tip", hg_repo, silent=True)
	tip = tip.split("\n")[0].split(":")[1].strip()
	return int(tip)

def hg_rename( hg_repo, from_file, to_file ):
	cmd("hg rename --after \"%s\" \"%s\"" % (from_file, to_file), hg_repo);
	
def hg_tag ( hg_repo, text, author, date ):
	old_tip = hg_tip(hg_repo)
	res = cmd("hg tag -u \"%s\" -d \"%s 0\" \"%s\""	 % (author, date, text), hg_repo)
	new_tip = hg_tip(hg_repo)
	if not new_tip == old_tip + 1:
		error("Mercurial tag did not work as expected: " + res)

def hg_handle_change( hg_repo, author, date, change, arg ):
	"""Processes a change event as output by darcs_changes_summary. These
	consist of file move/rename/add/delete commands."""
	if change == 'modify_file':
		pass
	elif change == 'add_file':
		pass
	elif change =='remove_file':
		pass
	elif change == 'add_directory':
		pass
	elif change == 'remove_directory':
		pass
	elif change == 'move':
		hg_rename(hg_repo, arg[0], arg[1])
	elif change == 'tag':
		hg_tag(hg_repo, arg, author, date)
	else:
		error('Unknown change type ' + change + ': ' + arg)

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
		skip       = None
	elif len(args) == 3:
		darcs_repo = os.path.abspath(args[0])
		hg_repo    = os.path.abspath(args[1])
		skip       = int(args[2])
	else:
		print USAGE
		sys.exit(-1)
	print 'This command is deprecated.  Use the convert extension instead.'
	# Initializes the target repo
	if not os.path.isdir(darcs_repo + "/_darcs"):
		print "No darcs directory found at: " + darcs_repo
		sys.exit(-1)
	if not os.path.isdir(hg_repo):
		os.mkdir(hg_repo)
	elif skip == None:
		print "Given HG repository must not exist when no SKIP is specified."
		sys.exit(-1)
	if skip == None:
		cmd("hg init \"%s\"" % (hg_repo))
		cmd("darcs initialize", hg_repo)
	# Get the changes from the Darcs repository
	change_number = 0
	for author, date, summary, chash, description in darcs_changes(darcs_repo):
		print "== changeset", change_number,
		if skip != None and change_number <= skip:
			print "(skipping)"
		else:
			text = summary + "\n" + description
			# The commit hash has a date like 20021020201112
			# --------------------------------YYYYMMDDHHMMSS
			date = chash.split("-")[0]
			epoch = int(mktime(strptime(date, '%Y%m%d%H%M%S')))
			darcs_pull(hg_repo, darcs_repo, chash)
			for change, arg in darcs_changes_summary(darcs_repo, chash):
				hg_handle_change(hg_repo, author, epoch, change, arg)
			hg_commit(hg_repo, text, author, epoch)
		change_number += 1
	print "Darcs repository (_darcs) was not deleted. You can keep or remove it."

# EOF
