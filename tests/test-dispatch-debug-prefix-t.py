# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# FIXME: "hg d" should not be ambiguous and list all debug commands.
sh % "newrepo"
sh % "hg d" == r"""
    hg: command 'd' is ambiguous:
    	dbsh or debugshell
    	debugancestor
    	debugapplystreamclonebundle
    	debugbuilddag
    	debugbundle
    	debugcapabilities
    	debugcheckcasecollisions
    	debugcheckoutidentifier
    	debugcheckstate
    	debugcolor
    	debugcommands
    	debugcomplete
    	debugconfig
    	debugcreatestreamclonebundle
    	debugdag
    	debugdata
    	debugdate
    	debugdeltachain
    	debugdiscovery
    	debugdrawdag
    	debugedenimporthelper
    	debugextensions
    	debugfilerevision
    	debugfileset
    	debugformat
    	debugfsinfo
    	debuggentrees
    	debuggetbundle
    	debugignore
    	debugindex
    	debugindexdot
    	debuginstall
    	debugknown
    	debuglabelcomplete
    	debuglocks
    	debugmergestate
    	debugmutation
    	debugmutationfromobsmarkers
    	debugnamecomplete
    	debugobsolete
    	debugpathcomplete
    	debugpickmergetool
    	debugprocesstree
    	debugprogress
    	debugpushkey
    	debugpvec
    	debugrebuildfncache
    	debugrebuildstate or debugrebuilddirstate
    	debugrename
    	debugrevlog
    	debugrevspec
    	debugsetparents
    	debugssl
    	debugstate or debugdirstate
    	debugstatus
    	debugstrip
    	debugsuccessorssets
    	debugtemplate
    	debugtreestate or debugtreedirstate
    	debugupdatecaches
    	debugupgraderepo
    	debugvisibility
    	debugwalk
    	debugwireargs
    	diff
    [255]"""
