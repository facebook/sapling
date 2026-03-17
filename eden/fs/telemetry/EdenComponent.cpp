/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenComponent.h"

#include <map>

namespace facebook::eden {

const std::map<EdenComponent, std::string_view> kComponentToString = {
    {EdenComponent::Fuse, "fuse"},
    {EdenComponent::Nfs, "nfs"},
    {EdenComponent::Prjfs, "prjfs"},
    {EdenComponent::Overlay, "overlay"},
    {EdenComponent::BackingStore, "backing_store"},
    {EdenComponent::ObjectStore, "object_store"},
    {EdenComponent::Thrift, "thrift"},
    {EdenComponent::Takeover, "takeover"},
    {EdenComponent::Privhelper, "privhelper"},
};

std::string_view toString(EdenComponent component) {
  return kComponentToString.at(component);
}

} // namespace facebook::eden
