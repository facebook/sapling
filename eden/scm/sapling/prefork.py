# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# If `prefork` is True, our process is going to fork(). Python modules can
# conditionalize based on `prefork` to avoid doing or initializing things that
# are not fork safe. In particular, before forking if we trigger any Rust code
# that initializes global or thread local state, starts threads, waits on
# mutexes, etc., we can have bad behavior post-fork.
prefork = False
