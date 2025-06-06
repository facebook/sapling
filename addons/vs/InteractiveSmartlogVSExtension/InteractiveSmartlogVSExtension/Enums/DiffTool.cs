/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System.ComponentModel;

namespace InteractiveSmartlogVSExtension
{
    public enum DiffTool
    {
        [Description("Visual Studio (internal)")]
        VisualStudio,
        p4merge,
        WinMerge,
        BeyondCompare,
        Custom,
    }
}
