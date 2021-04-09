# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Historically, support for EdenFS was provided via a separate Hg extension
# named "eden", so "eden" is what was added to the ".hg/requires" file.
# Going forward, it would be more appropriate to name the requirement "edenfs",
# but we need to run a Hypershell job to update the existing Eden checkouts.
requirement = "eden"
