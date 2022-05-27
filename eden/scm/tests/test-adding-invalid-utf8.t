#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#require py2

#require no-windows no-osx

# Test that trying to add invalid utf8 files to the repository will fail.

  $ hg init

# BEGIN OF NOT TRANSLATED
open("\x9d\xc8\xac\xde\xa1\xee", "wb").write("test")
# END OF NOT TRANSLATED


#if fsmonitor
  $ hg status
  skipping invalid utf-8 filename: 'È¬Þ¡î'
  $ hg addremove
  $ hg commit -m 'adding a filename that is invalid utf8'
  nothing changed
  [1]
#else
# This is different from the fsmonitor output above because the Rust walker error
# reporting escapes the invalid unicode characters with unicode codepoint \ufffd
# (which encodes to bytes \xef\xbf\xbd).
sh % "hg status" == "skipping invalid utf-8 filename: '\xef\xbf\xbd\xc8\xac\xde\xa1\xef\xbf\xbd'"
sh % "hg addremove" == "skipping invalid utf-8 filename: '\xef\xbf\xbd\xc8\xac\xde\xa1\xef\xbf\xbd'"
sh % "hg commit -m 'adding a filename that is invalid utf8'" == r"""
    skipping invalid utf-8 filename: '�Ȭޡ�'
    skipping invalid utf-8 filename: '�Ȭޡ�'
    nothing changed
    [1]"""
#endif
