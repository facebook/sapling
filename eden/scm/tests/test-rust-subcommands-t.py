# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# debug-args

sh % "hg --cwd . debug-args a b" == '["a", "b"]'
sh % "hg --cwd . debug args a b" == '["a", "b"]'
sh % "hg --cwd . debug --args a b" == '["a", "b"]'

# Aliases

sh % "hg --config 'alias.foo-bar=debug-args alias-foo-bar' foo bar 1 2" == '["alias-foo-bar", "1", "2"]'
sh % "hg --config 'alias.foo-bar=debug-args alias-foo-bar' foo-bar 1 2" == '["alias-foo-bar", "1", "2"]'
sh % "hg --config 'alias.foo-bar=debug-args alias-foo-bar' foo --bar 1 2" == '["alias-foo-bar", "1", "2"]'

# If both "foo-bar" and "foo" are defined, then "foo bar" does not resolve to
# "foo-bar".
#
# This is because: Supose we have "add" and "add-only-text" command.
# If the user has a file called "only-text", "add only-text" should probably
# use the "add" command.

sh % "hg --config 'alias.foo-bar=debug-args alias-foo-bar' --config 'alias.foo=debug-args alias-foo' foo bar" == '["alias-foo", "bar"]'
sh % "hg --config 'alias.foo-bar=debug-args alias-foo-bar' --config 'alias.foo=debug-args alias-foo' foo --bar" == '["alias-foo-bar"]'
sh % "hg --config 'alias.foo-bar=debug-args alias-foo-bar' --config 'alias.foo=debug-args alias-foo' foo-bar" == '["alias-foo-bar"]'
