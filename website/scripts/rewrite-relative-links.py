#!python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import glob
import os
import re


def rewrite_file(path):
    with open(path, "rb") as f:
        content = f.read().decode()

    def _replace(match):
        desc = match.group(1)
        link = match.group(2)
        if "#" in link:
            rel_path, hash = link.split("#", 1)
        else:
            rel_path, hash = link, ""
        full_path = os.path.join(os.path.dirname(path), rel_path)
        if not full_path.endswith(".md"):
            full_path += ".md"
        if os.path.exists(full_path):
            new_link = os.path.normpath(full_path)
            if hash:
                new_link += f"#{hash}"
            new_link = "/" + new_link
            if new_link != link:
                print(f" {link} => {new_link}")
                return f"[{desc}]({new_link})"
        print(f" {link} (unchanged)")
        return match.group(0)

    print(f"{path}:")
    new_content = re.subn(
        r"\[([^\]]+)\]\((?!/)([^):]+)\)", _replace, content, count=1000
    )[0]
    if new_content == content:
        print(" not changed")
    else:
        with open(path, "wb") as f:
            f.write(new_content.encode())
        print(" updated")


def main():
    os.chdir(os.path.dirname(os.path.dirname(os.path.realpath(__file__))))

    for path in glob.glob("docs/**/*.md", recursive=True):
        rewrite_file(path)


if __name__ == "__main__":
    main()
