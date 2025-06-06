/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;

namespace InteractiveSmartlogVSExtension.Models
{
    public static class SmartlogUrlCache
    {
        public static string LastComputedUrl { get; set; }
        public static string LastError { get; set; }
        public static DateTime LastComputedTimestamp { get; set; }
    }
}

