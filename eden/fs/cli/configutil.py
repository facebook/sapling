#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


import collections
import configparser
import errno
import os
import sys
from pathlib import Path
from typing import (
    Any,
    cast,
    DefaultDict,
    Dict,
    List,
    Mapping,
    MutableMapping,
    Optional,
    Sequence,
    Tuple,
    Type,
    TYPE_CHECKING,
    TypeVar,
    Union,
)

import toml

from .configinterpolator import EdenConfigInterpolator


if TYPE_CHECKING:

    class Strs(Tuple[str, ...]):
        pass

else:

    class Strs(tuple):
        pass


ConfigValue = Union[bool, str, Strs]
ConfigSectionName = str
ConfigOptionName = str
# pyre-fixme[33]: Aliased annotation cannot be `Any`.
_UnsupportedValue = Any

_TConfigValue = TypeVar("_TConfigValue", bound=ConfigValue)


class EdenConfigParser:
    _interpolator: configparser.Interpolation
    _sections: DefaultDict[
        ConfigSectionName, Dict[ConfigOptionName, Union[ConfigValue, _UnsupportedValue]]
    ]

    def __init__(self, interpolation: Optional[EdenConfigInterpolator] = None) -> None:
        super().__init__()
        self._interpolator = (
            configparser.Interpolation() if interpolation is None else interpolation
        )
        self._sections = collections.defaultdict(dict)

    def read_dict(
        self, dictionary: Mapping[ConfigSectionName, Mapping[ConfigOptionName, Any]]
    ) -> None:
        section = option = value = None
        try:
            for section, options in dictionary.items():
                for option, value in options.items():
                    self._sections[section][option] = self._make_storable_value(
                        section, option, value
                    )
        except AttributeError:
            raise Exception(
                "Malformed config. Config files use TOML format.\n"
                f"Issue found near section: {section}, option: {option}, value: {value}."
            )

    # Convert the passed EdenConfigParser to a raw dictionary (without
    # interpolation)
    # Useful for updating configuration files in different formats.
    # pyre-fixme[24]: Generic type `collections.OrderedDict` expects 2 type parameters.
    def to_raw_dict(self) -> collections.OrderedDict:
        # pyre-fixme[24]: Generic type `collections.OrderedDict` expects 2 type
        #  parameters.
        rslt: "collections.OrderedDict" = collections.OrderedDict()
        for section, options in self._sections.items():
            rslt[section] = collections.OrderedDict(options)
        return rslt

    def sections(self) -> List[ConfigSectionName]:
        return list(self._sections.keys())

    def get_section_str_to_any(self, section: ConfigSectionName) -> Mapping[str, Any]:
        options = self._get_raw_section(section)
        return {
            option: self._interpolate_value(
                section=section,
                option=option,
                value=self._ensure_value_is_supported(
                    section=section, option=option, value=value
                ),
            )
            for option, value in options.items()
        }

    def get_section_str_to_str(self, section: ConfigSectionName) -> Mapping[str, str]:
        options = self._get_raw_section(section)
        return {
            option: self._value_with_type(
                section=section, option=option, value=value, expected_type=str
            )
            for option, value in options.items()
        }

    def _get_raw_section(
        self, section: ConfigSectionName
    ) -> Mapping[ConfigSectionName, Union[ConfigValue, _UnsupportedValue]]:
        options = self._sections.get(section)
        if options is None:
            raise configparser.NoSectionError(section)
        return options

    def get_bool(
        self, section: ConfigSectionName, option: ConfigOptionName, default: bool
    ) -> bool:
        return self._get(section, option, default=default, expected_type=bool)

    def get_str(
        self, section: ConfigSectionName, option: ConfigOptionName, default: str
    ) -> str:
        return self._get(section, option, default=default, expected_type=str)

    def get_strs(
        self,
        section: ConfigSectionName,
        option: ConfigOptionName,
        default: Sequence[str],
    ) -> Strs:
        default_strs = Strs(default)
        return self._get(section, option, default=default_strs, expected_type=Strs)

    def _get(
        self,
        section: ConfigSectionName,
        option: ConfigOptionName,
        default: _TConfigValue,
        expected_type: Type[_TConfigValue],
    ) -> _TConfigValue:
        options = self._sections.get(section)
        if options is None:
            return default
        if option not in options:
            return default
        value = options[option]
        return self._value_with_type(
            section=section, option=option, value=value, expected_type=expected_type
        )

    def _value_with_type(
        self,
        section: ConfigSectionName,
        option: ConfigOptionName,
        value: Union[ConfigValue, _UnsupportedValue],
        expected_type: Type[_TConfigValue],
    ) -> _TConfigValue:
        # TODO(T39124448): Remove Pyre workaround; use isinstance directly.
        is_instance = isinstance
        if not is_instance(value, expected_type):
            expected_type_temp: Type[ConfigValue] = expected_type
            raise UnexpectedType(
                section=section,
                option=option,
                value=value,
                expected_type=expected_type_temp,
            )
        return self._interpolate_value(  # type: ignore  # T39124448, T39125053
            section=section, option=option, value=value
        )

    def has_section(self, section: ConfigSectionName) -> bool:
        return section in self._sections

    def __setitem__(
        self,
        section: ConfigSectionName,
        options: Mapping[ConfigOptionName, ConfigValue],
    ) -> None:
        self._sections[section] = dict(options)

    @property
    def _defaults(self) -> Mapping[ConfigOptionName, str]:
        return {}

    @property
    def _parser(
        self,
    ) -> MutableMapping[ConfigSectionName, Mapping[ConfigOptionName, str]]:
        return {}

    def _ensure_value_is_supported(
        self,
        section: ConfigSectionName,
        option: ConfigOptionName,
        value: Union[ConfigValue, _UnsupportedValue],
    ) -> ConfigValue:
        if not isinstance(value, (bool, str, Strs)):
            raise UnexpectedType(
                section=section, option=option, value=value, expected_type=None
            )
        return value

    def _interpolate_value(
        self, section: ConfigSectionName, option: ConfigOptionName, value: _TConfigValue
    ) -> _TConfigValue:
        if isinstance(value, Strs):
            return Strs(  # type: ignore  # T39125053
                self._interpolate_value(section, option, item) for item in value
            )
        elif isinstance(value, str):
            return self._interpolator.before_get(  # type: ignore  # T39125053
                self._parser, section, option, value, self._defaults
            )
        else:
            return value

    def _make_storable_value(
        self,
        section: ConfigSectionName,
        option: ConfigOptionName,
        # pyre-fixme[2]: Parameter annotation cannot be `Any`.
        value: Any,
    ) -> Union[ConfigValue, _UnsupportedValue]:
        if isinstance(value, (bool, str)):
            return value
        if isinstance(value, Sequence):
            if all(isinstance(item, str) for item in value):
                items = cast(Sequence[str], value)
                return Strs(items)
        return value


class UnexpectedType(Exception):
    section: ConfigSectionName
    option: ConfigOptionName
    # pyre-fixme[4]: Attribute annotation cannot be `Any`.
    value: Any
    expected_type: Optional[Type[ConfigValue]]

    def __init__(
        self,
        section: ConfigSectionName,
        option: ConfigOptionName,
        # pyre-fixme[2]: Parameter annotation cannot be `Any`.
        value: Any,
        expected_type: Optional[Type[ConfigValue]],
    ) -> None:
        super().__init__()
        self.section = section
        self.option = option
        self.value = value
        self.expected_type = expected_type

    def __str__(self) -> str:
        if self.expected_type is None:
            return (
                f"Unexpected {self.human_value_type} for "
                f"{self.section}.{self.option}: {self.human_value}"
            )
        else:
            return (
                f"Expected {self.human_expected_type} for "
                f"{self.section}.{self.option}, but got "
                f"{self.human_value_type}: {self.human_value}"
            )

    @property
    def human_expected_type(self) -> str:
        assert self.expected_type is not None
        return _toml_type_name(self.expected_type)

    @property
    def human_value_type(self) -> str:
        return _toml_type_name(type(self.value))

    @property
    def human_value(self) -> str:
        return _toml_value(self.value)


# pyre-fixme[24]: Generic type `type` expects 1 type parameter, use
#  `typing.Type[<base type>]` to avoid runtime subscripting errors.
def _toml_type_name(type: Type) -> str:
    if type is Strs:
        return "array of strings"
    if type is bool:
        return "boolean"
    if type is list:
        return "array"
    if type is str:
        return "string"
    return type.__name__


def _toml_value(value: Union[bool, str]) -> str:
    # pyre-fixme[24]: Generic type `type` expects 1 type parameter, use
    #  `typing.Type[<base type>]` to avoid runtime subscripting errors.
    TomlEncoder: Type = toml.TomlEncoder
    value_toml: str = TomlEncoder().dump_inline_table(value)
    return value_toml.rstrip()


# Lightweight Eden config reading without needing EdenInstance
# Used by tools like edenfs_config_manager that need to read telemetry
# config before EdenFS is running


def _get_default_etc_eden_dir() -> Path:
    if sys.platform == "win32":
        return Path("C:\\ProgramData\\facebook\\eden")
    else:
        return Path("/etc/eden")


def _get_default_home_dir() -> Path:
    # Delegate to util.get_home_dir() as the single source of truth. Import it
    # lazily: util pulls in heavy dependencies (thrift clients, subprocess, ...)
    # and this module is meant to stay a lightweight config reader.
    from . import util

    return util.get_home_dir()


def get_eden_config_paths(
    etc_eden_dir: Optional[Path] = None, home_dir: Optional[Path] = None
) -> List[Path]:
    """
    Get list of Eden config files in load order, without needing EdenInstance.
    Returns paths to: /etc/eden/config.d/*.toml, /etc/eden/edenfs.rc,
    /etc/eden/edenfs_dynamic.rc, ~/.edenrc

    Missing files are included in the list – caller should handle FileNotFoundError
    when reading them, matching EdenInstance.read_configs() behavior.
    """
    if etc_eden_dir is None:
        etc_eden_dir = _get_default_etc_eden_dir()
    else:
        etc_eden_dir = Path(etc_eden_dir)

    if home_dir is None:
        home_dir = _get_default_home_dir()
    else:
        home_dir = Path(home_dir)

    result: List[Path] = []
    config_d = etc_eden_dir / "config.d"
    try:
        rc_entries = os.listdir(config_d)
    except OSError as ex:
        if ex.errno != errno.ENOENT:
            raise
        rc_entries = []

    for name in rc_entries:
        if not name.startswith(".") and name.endswith(".toml"):
            result.append(config_d / name)

    result.sort()
    result.append(etc_eden_dir / "edenfs.rc")
    result.append(etc_eden_dir / "edenfs_dynamic.rc")
    result.append(home_dir / ".edenrc")

    return result


def load_eden_config(
    etc_eden_dir: Optional[Path] = None,
    home_dir: Optional[Path] = None,
    interpolation_dict: Optional[Dict[str, str]] = None,
) -> EdenConfigParser:
    """
    Load Eden config from standard locations without needing EdenInstance.
    This is useful for tools like edenfs_config_manager that need to read
    telemetry config before EdenFS is running.
    """
    # Build interpolation dict if not provided
    if interpolation_dict is None:
        if home_dir is None:
            home_dir_path = _get_default_home_dir()
        else:
            home_dir_path = Path(home_dir)

        if sys.platform == "win32":
            user_name = os.environ.get("USERNAME", "")
            user_id = 0
        else:
            user_name = os.environ.get("USER", "")
            user_id = os.getuid()

        interpolation_dict = {
            "USER": user_name,
            "USER_ID": str(user_id),
            "HOME": str(home_dir_path),
        }

    parser = EdenConfigParser(interpolation=EdenConfigInterpolator(interpolation_dict))

    for path in get_eden_config_paths(etc_eden_dir, home_dir):
        try:
            with path.open("r") as f:
                toml_cfg = toml.load(f)
            parser.read_dict(toml_cfg)
        except FileNotFoundError:
            # Ignore missing config files, matching EdenInstance.read_configs()
            continue
        except Exception:
            # Silently ignore parse errors to match EdenInstance behavior
            # (it logs a warning, but we don't have a logger here)
            continue

    return parser


def get_config_value(
    key: str,
    default: str = "",
    etc_eden_dir: Optional[Path] = None,
    home_dir: Optional[Path] = None,
    parser: Optional[EdenConfigParser] = None,
) -> str:
    """Get a string config value from Eden config files without needing EdenInstance.

    Pass an already-loaded ``parser`` to avoid re-reading and re-parsing every
    Eden config file when reading multiple values in a single run.
    """
    if parser is None:
        parser = load_eden_config(etc_eden_dir, home_dir)
    try:
        section, option = key.split(".", 1)
    except ValueError:
        return default
    return parser.get_str(section, option, default=default)


def get_config_bool(
    key: str,
    default: bool = False,
    etc_eden_dir: Optional[Path] = None,
    home_dir: Optional[Path] = None,
    parser: Optional[EdenConfigParser] = None,
) -> bool:
    """Get a bool config value from Eden config files without needing EdenInstance.

    Pass an already-loaded ``parser`` to avoid re-reading and re-parsing every
    Eden config file when reading multiple values in a single run.
    """
    if parser is None:
        parser = load_eden_config(etc_eden_dir, home_dir)
    try:
        section, option = key.split(".", 1)
    except ValueError:
        return default
    return parser.get_bool(section, option, default=default)
