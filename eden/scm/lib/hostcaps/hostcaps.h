/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

// @dep=//eden/scm/lib/hostcaps:hostcaps

extern "C" bool eden_is_prod();
extern "C" bool eden_has_servicerouter();
