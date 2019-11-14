# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init"
sh % "echo a" > "a"
sh % "hg ci -Am t" == "adding a"

sh % "hg mv a b"
sh % "hg ci -Am t1"
sh % "hg debugrename b" == "b renamed from a:b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3"

sh % "hg mv b a"
sh % "hg ci -Am t2"
sh % "hg debugrename a" == "a renamed from b:37d9b5d994eab34eda9c16b195ace52c7b129980"

sh % "hg debugrename --rev 1 b" == "b renamed from a:b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3"
