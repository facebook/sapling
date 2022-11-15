#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This script extract content for the website on each of the specified Sapling
commands.

This script takes the commands you want documentation for as an argument on
the command line. The argument should be a json encoded list of commands.

This script should be run with `hg debugshell`. i.e.
```
$ hg debugshell extract-command-documentation.py '["command1", "command2"]'
```
You can use a development version of sapling to run this command and you
will have access to the features and documentation included in that
development version of sapling. If you are building sapling via the
`make local` method. That would just mean running this from the root of the
sapling project folder:
```
$ ./hg debugshell generate-command-content.py '["command1", "command2"]'
```

This script outputs the documentation in the form of a json to stdout.

The json has a toplevel mapping from the name of a command to the
information about that command. We use the name that was specified in the input
in the mapping. The information about the command is a json object containing:
- an `aliases` mapping that contains the list of all the names for
  this command.
- a `doc` mapping that contains the rst (reStructuredText) blob of the
  descrition of the command.
- an `args` mapping which is a list of json objects for each argument to
  the command. Each argument object contains the `shortname` or single
  character that can be used for the argument. The `fullname` or long name of
  the argument. The `default` value for the argument if there is one. And
  finally a `description` of the argument.
"""

import json
import os
import sys
from dataclasses import dataclass
from typing import Dict, List, Optional, Tuple


# Tuple values: (shortname, fullname, default, arg_description)
Args = List[Tuple[str, str, str, str]]


@dataclass
class Command:
    name: str
    aliases: List[str]
    docstring: str
    args: Args
    subcommands: Optional[List["Command"]]

    def __init__(
        self, name: str, aliases: List[str], docstring: str, args=Args, subcommands=None
    ):
        self.name = name
        self.aliases = aliases
        self.docstring = docstring
        self.args = args
        self.subcommands = subcommands

    def to_dict(self):
        return {
            "name": self.name,
            "aliases": self.aliases,
            "doc": self.docstring,
            "args": [
                {
                    "shortname": arg[0],
                    "fullname": arg[1],
                    "default": arg[2],
                    "description": arg[3],
                }
                for arg in self.args
            ],
            "subcommands": [c.to_dict() for c in self.subcommands]
            if self.subcommands
            else None,
        }


def main():
    commands_table = e.commands.table  # noqa: F821
    commands_to_generate: List[str] = json.loads(sys.argv[1])
    commands_json = {}

    # first we split the alias so we can easier map a single alias into the key for
    # that command in the commands_table
    commands_to_alias: Dict[str, str] = {}
    for command in commands_table:
        for alias in command.split("|"):
            commands_to_alias[alias] = command

    for command in commands_to_generate:
        alias = commands_to_alias.get(command)
        if not alias:
            raise KeyError(f"no matching alias for command {command}")
        commands_json[command] = serialize_command_info_as_json(
            command, alias, commands_table[alias]
        )

    stdout = os.fdopen(1, "w")
    json.dump(commands_json, stdout)
    stdout.flush()
    # intentionally leaving stdout open so debugshell could write to it if it must.
    # this is better than cutting off an error message.


# Takes a command table maping of `alias` and `raw_info` and converts it
# into a json object to represent the serialized version of that object.
def serialize_command_info_as_json(name: str, alias: str, raw_info) -> Dict:
    aliases = alias.split("|")
    description_obj, python_args, *_ = raw_info
    command = extract_command(
        name, aliases=aliases, description_obj=description_obj, python_args=python_args
    )
    return command.to_dict()


# Documentation can be stored in different places and rust commands have their
# own specification of args.
def extract_command(
    name: str, aliases: List[str], description_obj, python_args: Args
) -> Command:
    if isinstance(description_obj, str):
        return Command(
            name, aliases=aliases, docstring=description_obj, args=python_args
        )
    if description_obj.__doc__ is not None:
        # Only Python currently supports subcommands.
        if description_obj.subcommands:
            # Subcommands for aliases are not supported yet.
            subcommands = [
                extract_command(
                    subcommand_name,
                    aliases=[],
                    description_obj=value[0],
                    python_args=value[1],
                )
                for subcommand_name, value in description_obj.subcommands.items()
            ]
        else:
            subcommands = None
        return Command(
            name,
            aliases=aliases,
            docstring=description_obj.__doc__,
            args=python_args,
            subcommands=subcommands,
        )
    elif hasattr(description_obj, "__rusthelp__"):
        return Command(
            name,
            aliases=aliases,
            docstring=description_obj.__rusthelp__[0],
            args=description_obj.__rusthelp__[1],
        )

    raise Exception(f"command `{name}` needs documentation")


main()
