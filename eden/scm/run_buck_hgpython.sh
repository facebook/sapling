# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

hg="$BUCK_DEFAULT_RUNTIME_RESOURCES/eden/scm/hg"

CHGDISABLE="${CHGDISABLE-1}"
export CHGDISABLE

exec -a python3 "$hg" "$@"
