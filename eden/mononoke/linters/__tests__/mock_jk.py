# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Mock jk CLI for external-linter-test sandbox.

Simulates `jk get <namespace>` with predefined knobs so the linter
can be tested end-to-end without a real JK backend.
"""

import sys

KNOWN_KNOBS = {
    "scm/mononoke": {
        "scm/mononoke:pushrebase_enable_merge_resolution": "True",
        "scm/mononoke:per_bookmark_locking": "True",
        "scm/mononoke:derived_data_use_content_manifests": "False",
    },
}


def main() -> None:
    if len(sys.argv) < 3 or sys.argv[1] != "get":
        print(f"Usage: {sys.argv[0]} get <namespace>", file=sys.stderr)
        sys.exit(1)

    namespace = sys.argv[2]
    knobs = KNOWN_KNOBS.get(namespace, {})

    for knob_name, value in knobs.items():
        print(f"{knob_name}: {value} (last modified: 2026-04-21 18:46:16 UTC)")

    sys.exit(0)


if __name__ == "__main__":
    main()
