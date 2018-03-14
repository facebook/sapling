# Commit cloud
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
""" sync changesets via the cloud

    [commitcloud]
    # type of commit cloud service to connect to
    servicetype = local

    # location of the commit cloud service to connect to
    servicelocation = /path/to/dir

    # hostname to use for the system
    hostname = myhost
"""

from __future__ import absolute_import

from . import commitcloudcommands

cmdtable = commitcloudcommands.cmdtable
