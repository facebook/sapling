#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


import abc
import argparse
import typing
from typing import Any, Awaitable, Callable, cast, Dict, List, Optional, Type, Union

from . import util


class CmdError(Exception):
    pass


class Subcmd(abc.ABC):
    NAME: Optional[str] = None
    HELP: Optional[str] = None
    DESCRIPTION: Optional[str] = None
    FORMATTER: Optional[argparse.HelpFormatter] = None
    ALIASES: Optional[List[str]] = None

    def __init__(self, parser: argparse.ArgumentParser) -> None:
        # Save a pointer to the parent ArgumentParser that this Subcmd belongs
        # to.  This is primarily useful for HelpCmd so it can show help
        # information about its sibling commands.
        self.parent_parser = parser

    # pyre-fixme[24]: Generic type `argparse._SubParsersAction` expects 1 type
    #  parameter.
    def add_parser(self, subparsers: argparse._SubParsersAction) -> None:
        # If get_help() returns None, do not pass in a help argument at all.
        # This will prevent the command from appearing in the help output at
        # all.
        kwargs: Dict[str, Any] = {"aliases": self.get_aliases()}
        help = self.get_help()
        if help is not None:
            # The add_parser() code checks if 'help' is present in the keyword
            # arguments.  Not being present is handled differently than if it
            # is present and None.  It only hides the command from the help
            # output if the 'help' argument is not present at all.
            kwargs["help"] = help
        description = self.get_description()
        if description is not None:
            kwargs["description"] = description
        formatter = self.get_formatter()
        if formatter is not None:
            kwargs["formatter_class"] = formatter
        parser = subparsers.add_parser(self.get_name(), **kwargs)
        parser.set_defaults(func=self.run)
        self.setup_parser(parser)

    def get_name(self) -> str:
        if self.NAME is None:
            raise NotImplementedError("Subcmd subclasses must set NAME")
        return self.NAME

    def get_help(self) -> Optional[str]:
        return self.HELP

    def get_description(self) -> Optional[str]:
        return self.DESCRIPTION

    def get_formatter(
        self,
    ) -> Optional[argparse.HelpFormatter]:
        return self.FORMATTER

    def get_aliases(self) -> List[str]:
        if self.ALIASES is None:
            return []
        return self.ALIASES[:]

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        # Subclasses should override setup_parser() if they have any
        # command line options or arguments.
        pass

    def add_subcommands(
        self,
        parser: argparse.ArgumentParser,
        cmds: List[Type["Subcmd"]],
        # pyre-fixme[24]: Generic type `argparse._SubParsersAction` expects 1 type
        #  parameter.
    ) -> argparse._SubParsersAction:
        return add_subcommands(parser, cmds)

    @abc.abstractmethod
    def run(self, args: argparse.Namespace) -> Union[Awaitable[int], int]:
        pass


CmdTable = List[Type[Subcmd]]


def subcmd(
    name: str,
    help: Optional[str] = None,
    aliases: Optional[List[str]] = None,
    cmd_table: Optional[CmdTable] = None,
) -> Callable[[Type[Subcmd]], Type[Subcmd]]:
    """
    @subcmd() is a decorator that can be used to help define Subcmd instances

    If the cmd_table argument is non-None the new Subcmd class will
    automatically be added to this list.

    Example usage:

        @subcmd('list', 'Show the result list')
        class ListCmd(Subcmd):
            def run(self, args: argparse.Namespace) -> int:
                # Perform the command actions here...
                pass
    """

    def wrapper(cls: Type[Subcmd]) -> Type[Subcmd]:
        # https://github.com/python/mypy/issues/2477
        # pyre-fixme[33]: Given annotation cannot be `Any`.
        cls_mypy: Any = cls

        class SubclassedCmd(cls_mypy):
            NAME = name
            HELP = help
            ALIASES = aliases

        if cmd_table is not None:
            # pyre-fixme[16]: Callable `subcmd` has no attribute `wrapper`.
            cmd_table.append(typing.cast(Type[Subcmd], SubclassedCmd))
        return typing.cast(Type[Subcmd], SubclassedCmd)

    return wrapper


class Decorator:
    """
    decorator() creates a new object that can act as a decorator function to
    help define Subcmd instances.

    This decorator object also maintains a list of all commands that have been
    defined using it.  This command list can later be passed to
    add_subcommands() to register these commands.
    """

    def __init__(self) -> None:
        self.commands: CmdTable = []

    def __call__(
        self, name: str, help: Optional[str], aliases: Optional[List[str]] = None
    ) -> Callable[[Type[Subcmd]], Type[Subcmd]]:
        return subcmd(name, help, aliases=aliases, cmd_table=self.commands)


def add_subcommands(
    parser: argparse.ArgumentParser,
    cmds: List[Type[Subcmd]],
    # pyre-fixme[24]: Generic type `argparse._SubParsersAction` expects 1 type parameter.
) -> argparse._SubParsersAction:
    # Sort the commands alphabetically.
    # The order they are added here is the order they will be displayed
    # in the --help output.
    # metavar replaces the long and ugly default list of subcommands on a
    # single line with a single COMMAND placeholder.  We still render the nicer
    # list below where we would have shown the nasty one.
    subparsers = parser.add_subparsers(metavar="COMMAND")
    for cmd_class in sorted(cmds, key=lambda c: c.NAME):
        # pyre-fixme[45]: Cannot instantiate abstract class `Subcmd`.
        cmd_instance = cmd_class(parser)
        cmd_instance.add_parser(subparsers)

    return subparsers


def _get_subparsers(
    parser: argparse.ArgumentParser,
    # pyre-fixme[24]: Generic type `argparse._SubParsersAction` expects 1 type parameter.
) -> Optional[argparse._SubParsersAction]:
    # pyre-fixme[33]: Given annotation cannot be `Any`.
    subparsers = cast(Any, parser)
    if subparsers is None:
        return None

    for action in subparsers._actions:
        if action.option_strings:
            continue
        if not isinstance(action, argparse._SubParsersAction):
            # This is not a subcommand
            return None
        return action

    return None


def do_help(parser: argparse.ArgumentParser, help_args: List[str]) -> int:
    # Figure out what subcommand we have been asked to show the help for.
    for idx, arg in enumerate(help_args):
        # pyre-fixme[33]: Given annotation cannot be `Any`.
        subcmds: Any = _get_subparsers(parser)
        if subcmds is None:
            cmd_so_far = " ".join(help_args[:idx])
            # The remaining arguments may be positional arguments
            # to this command.  Stop processing arguments here and show help
            # for the command we have processed so far.
            break

        subparser = subcmds.choices.get(arg, None)
        if not subparser:
            if idx == 0:
                util.print_stderr('error: unknown command "{}"', arg)
            else:
                cmd_so_far = " ".join(help_args[:idx])
                util.print_stderr(
                    'error: "{}" does not have a subcommand "{}"', cmd_so_far, arg
                )
            return 2

        parser = subparser

    parser.print_help()
    return 0


@subcmd("help", "Display command line usage information")
class HelpCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("args", nargs=argparse.ZERO_OR_MORE)

    def run(self, args: argparse.Namespace) -> int:
        return do_help(self.parent_parser, args.args)
