#!/usr/bin/env python
#
# Copyright 2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""prints repr(sys.stdin) but preserves newlines in input"""

import sys
print repr(sys.stdin.read())[1:-1].replace('\\n', '\n'),
