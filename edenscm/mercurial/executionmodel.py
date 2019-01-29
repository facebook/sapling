# Copyright 2018-present Facebook. All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

executedfrombinary = False


def setbinaryexecution(isbinary=False):
    global executedfrombinary
    executedfrombinary = isbinary
