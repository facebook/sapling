#require test-repo

  $ cd "`dirname "$TESTDIR"`"

look for python scripts that do not use /usr/bin/env

  $ hg files 'set:grep(r"^#!.*?python") and not grep(r"^#!/usr/bin/env python")'
  [1]

look for shell scripts that do not use /bin/sh

  $ hg files 'set:grep(r"^#!.*/bin/sh") and not grep(r"^#!/bin/sh")'
  [1]
