/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

namespace facebook {
namespace rust {
namespace srserver {

struct RustThriftMetadata;

} // namespace srserver
} // namespace rust

namespace scm {
namespace service {

rust::srserver::RustThriftMetadata* create_metadata() noexcept;

} // namespace service
} // namespace scm
} // namespace facebook
