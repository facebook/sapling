/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */


namespace InteractiveSmartlogVSExtension.Enums
{
    public enum ActionType
    {
        RenderISLView,

        OpenFile,

        // By default we will have the diff view rendered internally within VS editor
        OpenInternalDiffView,

        /**
         * Users can render the diff view in other tools they have configured like p4merge, WinMergeU, Bcomp etc.
         * They can select the diff view rendering option from Tools -> Options -> Interactive Smartlog
        */
        OpenExternalDiffView,

        RevertDiffChanges,
    }
}
