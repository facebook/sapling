/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Runtime.InteropServices;
using Microsoft.VisualStudio.Shell;

namespace InteractiveSmartlogVSExtension
{
    /// <summary>
    /// This class implements the tool window exposed by this package and hosts a user control.
    /// </summary>
    /// <remarks>
    /// In Visual Studio tool windows are composed of a frame (implemented by the shell) and a pane,
    /// usually implemented by the package implementer.
    /// <para>
    /// This class derives from the ToolWindowPane class provided from the MPF in order to use its
    /// implementation of the IVsUIElementPane interface.
    /// </para>
    /// </remarks>
    [Guid("c45777d1-c943-4a9e-8ba2-54f25df6b04c")]
    public class InteractiveSmartlogToolWindow : ToolWindowPane
    {
        /// <summary>
        /// Initializes a new instance of the <see cref="InteractiveSmartlogToolWindow"/> class.
        /// </summary>
        public InteractiveSmartlogToolWindow() : base(null)
        {
            Caption = "Interactive Smartlog";

            // This is the user control hosted by the tool window; Note that, even if this class implements IDisposable,
            // we are not calling Dispose on this object. This is because ToolWindowPane calls Dispose on
            // the object returned by the Content property.
            this.Content = new InteractiveSmartlogToolWindowControl();
        }
    }
}
