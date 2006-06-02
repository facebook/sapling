# hgweb/__init__.py - web interface to a mercurial repository
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.demandload import demandload
demandload(globals(), "mercurial.hgweb.hgweb_mod:hgweb")
demandload(globals(), "mercurial.hgweb.hgwebdir_mod:hgwebdir")
