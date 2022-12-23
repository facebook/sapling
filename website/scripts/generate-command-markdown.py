#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Dict, List

from builtin_alias import DEFAULT_ALIAS_DICT

"""
This script generates markdown pages for the docusaurus site for Sapling
commands.

Run this script as
```
$ ./generate-command-markdown.py
```

See
```
$ ./generate-command-markdown.py --help
```
For how to use this command.
"""

# Note that the documentation generator currently cannot extract docs for
# builtin-aliases: e.g. `bottom`, `restack`, and `top`. You have to add
# them manually in builtin_alias.DEFAULT_ALIAS_DICT
DEFAULT_COMMAND_LIST = [
    "absorb",
    "add",
    "addremove",
    "amend",
    "annotate",
    "backout",
    "bookmark",
    "clean",
    "clone",
    "commit",
    "config",
    "diff",
    "fold",
    "forget",
    "ghstack",
    "githelp",
    "goto",
    "graft",
    "help",
    "hide",
    "histedit",
    "init",
    "journal",
    "log",
    "metaedit",
    "next",
    "pr",
    "prev",
    "pull",
    "push",
    "rebase",
    "redo",
    "remove",
    # "restack",
    "revert",
    "root",
    "shelve",
    "show",
    "split",
    "status",
    "unamend",
    "uncommit",
    "undo",
    "unhide",
    "unshelve",
    "web",
]


def eprint(*args, **kwargs):
    print(*args, file=sys.stderr, **kwargs)


def get_sapling() -> str:
    # Use the SL_BIN environment variable to use a debug build of Sapling.
    env_sl_bin = os.environ.get("SL_BIN")
    return env_sl_bin if env_sl_bin else "sl"


subprocess_cwd = os.path.dirname(__file__)

# Extract command documentation from Sapling.
def generate_commands_json(command_list: List[str]) -> Dict:
    proc = subprocess.run(
        [
            get_sapling(),
            "debugshell",
            "extract-command-documentation.py",
            json.dumps(command_list, indent=2),
        ],
        capture_output=True,
        cwd=subprocess_cwd,
    )
    if proc.returncode != 0:
        eprint(f"extracting website contents failed: \n {proc}")
        proc.check_returncode()
    return json.loads(proc.stdout)


# Regenerate and recache command documentation from Sapling.
def regenerate_commands_json(command_list: List[str], cache_path: Path) -> Dict:
    eprint("extracting documentation from Sapling ...")
    commands_json = generate_commands_json(command_list)

    with open(cache_path, "w") as command_json_file:
        json.dump(commands_json, command_json_file, indent=2)

    return commands_json


# Get the command documentation extracted from Sapling from our on disk cache
# or regenerate it
def get_commands_json(extract_docs_from_sapling, command_list, command_docs_json_path):
    if not extract_docs_from_sapling:
        try:
            with open(command_docs_json_path, "r") as cached_commands_json_file:
                return json.load(cached_commands_json_file)
        except FileNotFoundError:
            # allows us to fall back to recreating the cache when it does not
            # exist.
            pass

    return regenerate_commands_json(command_list, command_docs_json_path)


# Uses mercurial's minirst module to render the rst. We might eventually wanna
# swap this for a prettier rst to markdown.
def rst_to_markdown(rsts: Dict[str, str]) -> Dict[str, str]:
    proc = subprocess.run(
        [
            get_sapling(),
            "debugshell",
            "rst-to-md.py",
        ],
        capture_output=True,
        input=json.dumps(rsts).encode("utf-8"),
        cwd=subprocess_cwd,
    )
    if proc.returncode != 0:
        eprint(f"converting rst to md failed: \n {proc}")
        proc.check_returncode()
    command_name_to_markdown = json.loads(proc.stdout)

    # Temporary workaround: a more comprehensive escaping strategy should be
    # done in rst-to-md.py.
    return {k: _escape_import_in_doc(v) for k, v in command_name_to_markdown.items()}


def _escape_import_in_doc(doc: str) -> str:
    lines = doc.split("\n")
    return "\n".join([_escape_import_in_line(line) for line in lines])


def _escape_import_in_line(line: str) -> str:
    if line.startswith("import"):
        return "&#x69;" + line[1:]
    else:
        return line


# Regenerate markdown from rst and recache it.
def translate_json_to_markdown(
    command_list, commands_json, command_docs_markdown_json_path
) -> Dict:
    eprint("translating rst to markdown ...")
    rsts_to_render = {}
    for name in command_list:
        command = commands_json.get(name)
        if not command:
            raise KeyError(
                f"No command information generated for {command}. "
                "Do you need to regenerate the commands.json? Try running this "
                "command again with out `--only-rerender`"
            )
        doc = command.get("doc")
        if doc:
            # Add bold emphasis to the command summary (which is the first line).
            lines = doc.split("\n")
            lines[0] = f"**{lines[0]}**"
            rsts_to_render[name] = "\n".join(lines)

        subcommands = command.get("subcommands")
        if subcommands:
            for sub in subcommands:
                doc = sub.get("doc")
                if doc:
                    rsts_to_render[f"{name}.{sub['name']}"] = doc

    markdowns = rst_to_markdown(rsts_to_render)

    for name in command_list:
        command = commands_json[name]
        if "doc" in command:
            command["doc"] = markdowns[name]

        subcommands = command.get("subcommands")
        if subcommands:
            for sub in subcommands:
                if "doc" in sub:
                    sub["doc"] = markdowns[f"{name}.{sub['name']}"]

    with open(command_docs_markdown_json_path, "w") as command_json_file:
        json.dump(commands_json, command_json_file, indent=2)

    return commands_json


def get_markdown_commands_json(
    rst_to_md, command_list, commands_json, command_docs_markdown_json_path
):
    if not rst_to_md:
        try:
            with open(
                command_docs_markdown_json_path, "r"
            ) as cached_commands_json_file:
                return json.load(cached_commands_json_file)
        except FileNotFoundError:
            # allows us to fall back to recreating the cache when it does not
            # exist.
            pass
    return translate_json_to_markdown(
        command_list, commands_json, command_docs_markdown_json_path
    )


# regenerate the command documentation page content.
def generate_pages(command_list, commands_json) -> Dict[str, str]:
    result = {}

    for index, name in enumerate(command_list):
        sidebar = f"""\
---
sidebar_position: {index}
---
"""
        signed_source = """\
<!--
  \x40generated <<SignedSource::*O*zOeWoEQle#+L!plEphiEmie@IsG>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->
"""

        command = commands_json.get(name)
        if not command:
            raise KeyError(
                f"No command information generated for {name}. "
                "Do you need to regenerate the commands.json? Try running this "
                "command again with `--full-build`"
            )

        alias = "## " + " | ".join(command["aliases"])

        doc = command["doc"]
        formatted_doc = doc if doc else "No description available."

        args = command["args"]
        arg_table = f"## arguments\n{create_arg_table(args)}" if args else ""

        subcommands = ""
        subcommands_json = command.get("subcommands")
        if subcommands_json:
            subcommands += "## subcommands\n"
            for sub in subcommands_json:
                subcommands += f"### {sub['name']}\n"
                subcommands += sub.get("doc", "") + "\n"
                args = sub.get("args")
                if args:
                    subcommands += create_arg_table(args)

        # Note that to workaround an issue in Docusaurus, signed_source has to
        # appear after alias so that alias shows in the preview on the category
        # page instead of the end of the HTML comment for signed_source.
        output = f"""\
{sidebar}
{alias}
{signed_source}
{formatted_doc}
{arg_table}
{subcommands}"""
        result[name] = output
    return result


def create_arg_table(args) -> str:
    arg_table = "| shortname | fullname | default | description |\n"
    arg_table += "| - | - | - | - |\n"
    for arg in args:
        description = arg["description"]
        if should_exclude_arg(description):
            continue

        cells = ["", "", "", ""]
        shortname = arg["shortname"]
        fullname = arg["fullname"]
        default = arg["default"]

        if shortname:
            cells[0] = f"`-{shortname}`"

        if fullname:
            cells[1] = f"`--{fullname}`"

        if default is not None and default != "" and default != []:
            # Exclude `None` because we have nothing to print in that case.
            # Exclude empty string because, in practice, it does not appear
            # to represent the default value, but signals that the option
            # takes a string as an argument.
            # Exclude empty list because it implies the option is repeatable,
            # which we should communicate in some other way.
            if isinstance(default, str):
                # `ghstack submit --message` has a non-empty string as a
                # default value.
                cells[2] = f"`{json.dumps(default)}`"
            elif isinstance(default, bool):
                cells[2] = f"`{str(default).lower()}`"
            else:
                cells[2] = f"`{default}`"

        if description:
            cells[3] = description

        arg_table += "".join([f"| {cell}" for cell in cells]) + "|\n"
    return arg_table.rstrip("\n")


def should_exclude_arg(description: str) -> bool:
    return (
        "(DEPRECATED)" in description
        or "(ADVANCED)" in description
        or "(EXPERIMENTAL)" in description
    )


# clear out all the documentation files for all the aliases of a command.
def remove_command_doc(aliases: List[str], output_dir: Path):
    for alias in aliases:
        try:
            os.remove(output_dir / f"{alias}.md")
        except FileNotFoundError:
            pass


# regenerate and write the command documentation pages to disk.
def regenerate_pages(command_list, commands_json, output_dir: Path):
    eprint("Regenerating documentation ...")
    command_markdown = generate_pages(command_list, commands_json)

    markdown_files = []
    for (command, info) in command_markdown.items():
        remove_command_doc(commands_json[command]["aliases"], output_dir)

        command_file_path = output_dir / f"{command}.md"
        with open(command_file_path, "w") as command_file:
            command_file.write(info)
        markdown_files.append(command_file_path)
    # Note that if the list of markdown_files exceeds ARG_MAX, we will either
    # have to write the list of paths to a file that signsource reads or
    # divide it up into batches. Running `yarn run` once per file is too slow.
    eprint("Signing files ...")
    subprocess.check_output(
        ["yarn", "run", "signsource"] + markdown_files, cwd=subprocess_cwd
    )

    with open(output_dir / "_category_.json", "w") as f:
        category = {
            "label": "Commands",
            "position": 4.5,
            "link": {"type": "generated-index"},
        }
        json.dump(category, f, indent=2)
        f.write("\n")


def _combine_commands_and_builtin_alias(command_list, commands_json, alias_dict):
    command_list = sorted(command_list + list(alias_dict.keys()))
    commands_json = {**commands_json, **alias_dict}
    return command_list, commands_json


def main(args):
    current_path = Path(__file__).parent
    docs_path = current_path / "../docs"
    command_docs_path = docs_path / "commands"
    command_docs_json_path = command_docs_path / "commands.json"
    command_docs_markdown_json_path = command_docs_path / "commands-markdown.json"

    command_list = DEFAULT_COMMAND_LIST
    if args.commands is not None:
        command_list = args.commands.split(",")

    extract_docs_from_sapling = args.extract_docs_from_sapling or args.full_build
    rst_to_md = args.rst_to_md or args.full_build
    rerender = args.rerender or args.full_build

    commands_json = get_commands_json(
        extract_docs_from_sapling, command_list, command_docs_json_path
    )

    if not rst_to_md and not rerender:
        return

    markdown_commands_json = get_markdown_commands_json(
        rst_to_md, command_list, commands_json, command_docs_markdown_json_path
    )

    if not rerender:
        return

    command_list, markdown_commands_json = _combine_commands_and_builtin_alias(
        command_list, commands_json, DEFAULT_ALIAS_DICT
    )

    regenerate_pages(command_list, markdown_commands_json, command_docs_path)


if __name__ == "__main__":
    string_default_allow_list = ",".join(DEFAULT_COMMAND_LIST)

    def formatter(prog):
        return argparse.HelpFormatter(
            prog, width=min(shutil.get_terminal_size().columns - 2, 80)
        )

    parser = argparse.ArgumentParser(
        description="""
Generate markdown pages for Sapling commands for the OSS docusaurus website.

By default this script will only regenerate the markdown from the latest
cached version of the hg documentation (in command-markdown.json). Though it
will build any missing caches.
Use `--full-build` to force a slower rebuild of the caches and markdown. You
could also rebuild the cache files with there coresponding options.

Commands markdown page files are placed in website/docs/commands/.
This script will create the files with the name specified in the default values
or on the command line. For example if you use "--commands checkout" you will
get a "checkout.md" file. If you use "--commands update" you will get an
"update.md" file.

Note that we will clear out .md files for other aliases of that command when
we create a new command file. For example, "--commands update" will delete a
"checkout.md" file. The ordering the commands are specified in will also
determine the sidebar positioning of each of the pages.
""",
        formatter_class=formatter,
    )
    parser.add_argument(
        "--extract-docs-from-sapling",
        action="store_true",
        help="Reextract the raw documentation out of Sapling for each of the "
        "commands. This recreates 'command.json'.",
    )
    parser.add_argument(
        "--rst-to-md",
        action="store_true",
        help="Reconvert the raw Sapling rst command description to markdown "
        "for each command. This recreates 'command-markdown.json'. ",
    )
    parser.add_argument(
        "--rerender",
        type=bool,
        default=True,
        help="Regenerate the website pages. This is probably the option you "
        "want for quick iteration on the formating of the webpages.",
    )
    parser.add_argument(
        "--full-build",
        action="store_true",
        help="Rebuild the website pages from scratch (extracts documentation "
        "out of sapling, converts the rst to markdown, and then generates the "
        "webpages).",
    )
    parser.add_argument(
        "--commands",
        "-c",
        type=lambda raw_arg: raw_arg.split(","),
        help="A comma delimited list of command names. This script will "
        "generate content/markdown only for these commands instead of the "
        f"default set of commands. The default is: {string_default_allow_list}",
        default=None,
    )
    args = parser.parse_args()

    main(args)

    eprint("SUCCESS")
