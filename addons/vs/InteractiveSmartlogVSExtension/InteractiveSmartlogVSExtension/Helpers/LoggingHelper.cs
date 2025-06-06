/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// This file is just adding some debugging logs for ISL users to help them troubleshoot issues.
// We are not collecting any user data/telemetry via this logging helper.

using System;
using System.Diagnostics;
using System.Threading.Tasks;
using System.Windows.Forms;
using Microsoft.VisualStudio.Shell.Interop;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio;

namespace InteractiveSmartlogVSExtension
{
    class LoggingHelper
    {
        private static IVsOutputWindowPane _outputPane;
        protected static IVsOutputWindowPane OutputPane => _outputPane ?? (_outputPane = GetOutputPane());

        public static void ShowMessage(string message)
        {
            MessageBox.Show(message, "Message from WebView", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }

        /// <summary>
        /// Writes a string to the output window of Visual Studio.
        /// </summary>
        /// <param name="text">The message to write to the window.</param>
        public static async Task WriteAsync(string text)
        {
            // Required for following calls to IVsOutputWindowPaneNoPump.OutputStringNoPump and IVsOutputWindowPane.OutputStringThreadSafe.
            await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync();

            if (OutputPane == null)
            {
                WriteToDebug("Error: Null OutputPane.");
                WriteToDebug(text);
                return;
            }

            // If available, use OutputStringNoPump, which is the safest way to write to the pane.
            if (OutputPane is IVsOutputWindowPaneNoPump pumplessPane)
            {
                pumplessPane.OutputStringNoPump(text + Environment.NewLine);
                return;
            }

            // Fall back to OutputStringThreadSafe, which is the second safest way to write to the pane.
            if (ErrorHandler.Failed(OutputPane.OutputStringThreadSafe(text + Environment.NewLine)))
            {
                WriteToDebug("Error: IVsOutputWindowPane.OutputStringThreadSafe faild.");
                WriteToDebug(text);
            }
        }

        /// <summary>
        /// Writes a message to the Debug output stream of the current process.
        /// </summary>
        /// <param name="text"></param>
        public static void WriteToDebug(string text)
        {
            Debug.WriteLine("ISL for Visual Studio fallback output: " + text);
        }

        /// <summary>
        /// Helper function used by the OutputPane property to get the output pane.
        /// </summary>
        private static IVsOutputWindowPane GetOutputPane()
        {
            ThreadHelper.ThrowIfNotOnUIThread();

            var outputWindow = (IVsOutputWindow)Package.GetGlobalService(typeof(SVsOutputWindow));
            if (outputWindow == null)
            {
                WriteToDebug("Cannot get the SVsOutputWindow service.");
                return null;
            }

            var paneGuid = Guid.Parse("{FDBE8FB2-6D31-4B1F-88F9-CAD6244D81E5}");
            var paneName = "ISL for Visual Studio";

            if (ErrorHandler.Failed(outputWindow.GetPane(ref paneGuid, out var pane)) || (pane == null))
            {
                if (ErrorHandler.Failed(outputWindow.CreatePane(ref paneGuid, paneName, fInitVisible: 1, fClearWithSolution: 0)))
                {
                    WriteToDebug("Failed to create the Output window pane.");
                    return null;
                }
                if (ErrorHandler.Failed(outputWindow.GetPane(ref paneGuid, out pane)) || (pane == null))
                {
                    WriteToDebug("Failed to get the Output window pane.");
                    return null;
                }
            }

            return pane;
        }
    }
}
