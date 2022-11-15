#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import json
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--repo",
        metavar="[HOST/]OWNER/REPO",
        help="GitHub repository",
        type=str,
        required=True,
    )
    parser.add_argument(
        "--out",
        type=str,
        help="where to write the generated .ts file",
        required=True,
    )
    args = parser.parse_args()
    assets_json = subprocess.check_output(
        [
            "gh",
            "release",
            "view",
            "--repo",
            args.repo,
            "--json",
            "assets",
        ]
    )
    assets = json.loads(assets_json)
    ts_contents = f"""\
/\x2a\x2a
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

/*
 * \x40generated <<SignedSource::*O*zOeWoEQle#+L!plEphiEmie@IsG>>
 * Run `./scripts/gen_release_data.py` to regenerate.
 */

export const latestReleaseAssets = {json.dumps(assets, indent=2)};
"""
    with open(args.out, "w") as f:
        f.write(ts_contents)
    subprocess.check_output(["yarn", "run", "signsource", args.out])


if __name__ == "__main__":
    main()
