# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import sys

isposix = os.name == "posix"
isdarwin = sys.platform == "darwin"
islinux = sys.platform.startswith("linux")
iswindows = os.name == "nt"
