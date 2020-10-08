# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os
import sys
from typing import List, Set

from facebook.eden.ttypes import GlobParams

from . import subcmd as subcmd_mod, tabulate
from .cmd_util import get_eden_instance, require_checkout
from .config import EdenCheckout, EdenInstance
from .subcmd import Subcmd


prefetch_profile_cmd = subcmd_mod.Decorator()

# find the profile inside the given checkout and return a set of its contents.
def get_contents_for_profile(
    checkout: EdenCheckout, profile: str, silent: bool
) -> Set[str]:
    k_relative_profiles_location = "tools/scm/prefetch_profiles/profiles"

    profile_path = checkout.path / k_relative_profiles_location / profile

    if not profile_path.is_file():
        if not silent:
            print(f"Profile {profile} not found for checkout {checkout.path}.")
        return set()

    with open(profile_path) as f:
        return {pat.strip() for pat in f.readlines()}


# prefetch all of the files specified by a profile in the given checkout
def prefetch_profiles(
    checkout: EdenCheckout,
    instance: EdenInstance,
    profiles: List[str],
    enable_prefetch: bool,
    silent: bool,
):
    all_profile_contents = set()

    for profile in profiles:
        all_profile_contents |= get_contents_for_profile(checkout, profile, silent)

    with instance.get_thrift_client_legacy() as client:
        return client.globFiles(
            GlobParams(
                mountPoint=bytes(checkout.path),
                globs=list(all_profile_contents),
                includeDotfiles=False,
                prefetchFiles=enable_prefetch,
                suppressFileList=silent,
            )
        )


@prefetch_profile_cmd("record", "Start recording fetched file paths.")
class RecordProfileCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        with instance.get_thrift_client_legacy() as client:
            client.startRecordingBackingStoreFetch()
        return 0


@prefetch_profile_cmd(
    "finish",
    "Stop recording fetched file paths and save previously"
    " collected fetched file paths in the output prefetch profile",
)
class FinishProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--output-path",
            nargs="?",
            help="The output path to store the prefetch profile",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        with instance.get_thrift_client_legacy() as client:
            files = client.stopRecordingBackingStoreFetch()
            output_path = (
                args.output_path
                if args.output_path
                else os.path.abspath("prefetch_profile.txt")
            )
            with open(output_path, "w") as f:
                for path in sorted(files.fetchedFilePaths["HgQueuedBackingStore"]):
                    f.write(os.fsdecode(path))
                    f.write("\n")
        return 0


@prefetch_profile_cmd(
    "list", "List all of the currenly activated prefetch profiles for a checkout."
)
class ListProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--checkout",
            help="The checkout for which you want to see all the profiles this profile.",
            default=None,
        )

    def run(self, args: argparse.Namespace) -> int:
        checkout = args.checkout

        instance, checkout, _rel_path = require_checkout(args, checkout)

        profiles = sorted(checkout.get_config().active_prefetch_profiles)

        columns = ["Name"]
        data = [{"Name": name} for name in profiles]

        print(tabulate.tabulate(columns, data))

        return 0


@prefetch_profile_cmd(
    "activate",
    "Tell EdenFS to smart prefetch the files specified by the prefetch profile."
    " (Eden will prefetch the files in this profile immediately, when checking "
    " out a new commit and for some commits on pull).",
)
class ActivateProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("profile_name", help="Profile to activate.")
        parser.add_argument(
            "--checkout",
            help="The checkout for which you want to activate this profile.",
            default=None,
        )

    def run(self, args: argparse.Namespace) -> int:
        checkout = args.checkout

        instance, checkout, _rel_path = require_checkout(args, checkout)

        return checkout.activate_profile(args.profile_name)


@prefetch_profile_cmd(
    "deactivate",
    "Tell EdenFS to STOP smart prefetching the files specified by the prefetch"
    " profile.",
)
class DeactivateProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("profile_name", help="Profile to activate.")
        parser.add_argument(
            "--checkout",
            help="The checkout for which you want to activate this profile.",
            default=None,
        )

    def run(self, args: argparse.Namespace) -> int:
        checkout = args.checkout

        instance, checkout, _rel_path = require_checkout(args, checkout)

        return checkout.deactivate_profile(args.profile_name)


@prefetch_profile_cmd(
    "fetch",
    "Prefetch all the active prefetch profiles or specified prefetch profiles. "
    "This is intended for use in after checkout and pull.",
)
class FetchProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--checkout",
            help="The checkout for which the profiles should be fetched.",
            default=None,
        )
        parser.add_argument(
            "--verbose",
            help="Print extra info including warnings and the names of the "
            "matching files to fetch.",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--skip-prefetch",
            help="Do not prefetch profiles only find all the files that match "
            "them. This will still list the names of matching files when the "
            "verbose flag is also used",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--profile-names",
            nargs="*",
            help="Fetch only these named profiles instead of the active set of "
            "profiles.",
            default=None,
        )

    def run(self, args: argparse.Namespace) -> int:
        if sys.platform == "win32":
            # TODO(kmancini) prefetch profiles is not supported on windows yet
            return 0

        checkout = args.checkout

        instance, checkout, _rel_path = require_checkout(args, checkout)

        if args.profile_names is not None:
            profiles_to_fetch = args.profile_names
        else:
            profiles_to_fetch = checkout.get_config().active_prefetch_profiles

        if not profiles_to_fetch:
            if args.verbose:
                print("No profiles to fetch.")
            return 0

        result = prefetch_profiles(
            checkout,
            instance,
            profiles_to_fetch,
            enable_prefetch=not args.skip_prefetch,
            silent=not args.verbose,
        )

        if args.verbose:
            for name in result.matchingFiles:
                print(os.fsdecode(name))
        return 0


class PrefetchProfileCmd(Subcmd):
    NAME = "prefetch_profile"
    HELP = "Create, manage, and use Prefetch Profiles. This command is "
    " primarily for use in automation."

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        self.add_subcommands(parser, prefetch_profile_cmd.commands)

    def run(self, args: argparse.Namespace) -> int:
        return 0
