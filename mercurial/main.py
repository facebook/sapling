# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import


if __name__ == "__main__":
    import hgdemandimport

    hgdemandimport.enable()
    from . import dispatch

    dispatch.run(entrypoint="mercurial.main")
