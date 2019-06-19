#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import collections
import configparser
from typing import (
    TYPE_CHECKING,
    Any,
    DefaultDict,
    Dict,
    List,
    Mapping,
    MutableMapping,
    Optional,
    Sequence,
    Tuple,
    Type,
    TypeVar,
    Union,
    cast,
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
        for section, options in dictionary.items():
            for option, value in options.items():
                self._sections[section][option] = self._make_storable_value(
                    section, option, value
                )

    # Convert the passed EdenConfigParser to a raw dictionary (without
    # interpolation)
    # Useful for updating configuration files in different formats.
    def to_raw_dict(self) -> collections.OrderedDict:
        rslt = collections.OrderedDict()  # type: collections.OrderedDict
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
            expected_type_temp: Type[ConfigValue] = expected_type  # type: ignore
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
        self
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
        self, section: ConfigSectionName, option: ConfigOptionName, value: Any
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
    value: Any
    expected_type: Optional[Type[ConfigValue]]

    def __init__(
        self,
        section: ConfigSectionName,
        option: ConfigOptionName,
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
    TomlEncoder: Type = toml.TomlEncoder  # type: ignore
    value_toml: str = TomlEncoder().dump_inline_table(value)
    return value_toml.rstrip()
