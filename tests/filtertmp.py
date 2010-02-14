#!/usr/bin/env python
#
# This used to be a simple sed call like:
#
#  $ sed "s:$HGTMP:*HGTMP*:"
#
# But $HGTMP has ':' under Windows which breaks the sed call.
#
import sys, os

input = sys.stdin.read()
input = input.replace(os.environ['HGTMP'], '$HGTMP')
input = input.replace(os.sep, '/')
sys.stdout.write(input)
