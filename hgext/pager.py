# pager.py - display output using a pager
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
# To load the extension add it to your .hgrc file
#
#   [extension]
#   hgext.pager =
#
# To set the pager that should be used, set the application variable
#
#   [pager]
#   application = less
#
# You can also set environment variables there
#
#   [pager]
#   application = LESS='FSRX' less
#
# If no application is set, the pager extensions use the environment
# variable $PAGER. If neither pager.application, nor
# $PAGER is set, no pager is used.
#
# If you notice "BROKEN PIPE" error messages, you can disable them
# by setting
#
#  [pager]
#  quiet = True
#

import sys, os, signal

def getpager(ui):
    '''return a pager

    We separate this method from the pager class as we don't want to
    instantiate a pager if it is not used at all
    '''
    if sys.stdout.isatty():
        return (ui.config("pager", "application")
                or os.environ.get("PAGER"))

def uisetup(ui):
    # disable broken pipe error messages
    if ui.configbool('pager', 'quiet', False):
        signal.signal(signal.SIGPIPE, signal.SIG_DFL)

    if getpager(ui):
        pager = os.popen(getpager(ui), 'wb')
        sys.stderr = pager
        sys.stdout = pager
