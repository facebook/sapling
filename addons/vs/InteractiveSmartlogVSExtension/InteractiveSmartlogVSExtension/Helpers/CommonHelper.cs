/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */


using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Threading.Tasks;
using System.Xml;
using EnvDTE;
using EnvDTE80;
using InteractiveSmartlogVSExtension.Enums;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio.Shell.Interop;

namespace InteractiveSmartlogVSExtension.Helpers
{
    public class CommonHelper
    {
        // @fb-only: private static TelemetryLogger telemetryLogger = new TelemetryLogger();

        public struct WindowId
        {
            public static readonly string Value = GetCurrentWindowId() ?? Guid.NewGuid().ToString();
        }

        /// <summary>
        /// Gets the current window ID.
        /// </summary>
        /// <returns></returns>
        public static string GetCurrentWindowId()
        {
            try
            {
                ThreadHelper.ThrowIfNotOnUIThread();
                DTE2 dte = (DTE2)Package.GetGlobalService(typeof(DTE));
                if (dte != null && dte.ActiveWindow != null)
                {
                    string windowId = dte.ActiveWindow.ObjectKind.Trim('{', '}');
                    return windowId;
                }
            }
            catch (Exception ex)
            {
                LoggingHelper.WriteToDebug("Failed to get current window ID: " + ex.Message);
            }
            return null;
        }

        /// <summary>
        /// Get the username of from the CPE machine, environment variable.
        /// </summary>
        /// <returns>A string </returns>
        public static string GetUserName()
        {
            return Environment.UserName;
        }

        /// <summary>
        /// Get the hostname of from the CPE machine, environment variable.
        /// </summary>
        /// <returns>The hostname as a string.</returns>
        public static string GetHostName()
        {
            return System.Net.Dns.GetHostName();
        }
        public static long GetTime()
        {
            return DateTimeOffset.Now.ToUnixTimeSeconds();
        }

        // With this corrected code block:
        public static string GetIDEName()
        {
            string iDEName = "Unknown";
            try
            {
                using (var proc = System.Diagnostics.Process.GetCurrentProcess()) // Fully qualify Process
                {
                    var versionInfo = proc?.MainModule?.FileVersionInfo;

                    iDEName = versionInfo?.ProductName ?? "Unknown";
                    iDEName = Regex.Replace(iDEName, @"[^a-zA-Z0-9\s]", "");
                }
            }
            catch (Exception ex)
            {
                LoggingHelper.WriteAsync($"Failed to get the IDE name: {ex.Message}"); // fb-only
            }

            return iDEName;
        }

        public static string GetIDEVersion()
        {
            string iDEVersion = "Unknown";
            try
            {
                var proc = System.Diagnostics.Process.GetCurrentProcess();
                var versionInfo = proc?.MainModule?.FileVersionInfo;

                iDEVersion = versionInfo?.ProductVersion ?? "Unknown";
            }
            catch (Exception ex)
            {
                LoggingHelper.WriteAsync($"Failed to get the IDE version: {ex.Message}");
            }

            return iDEVersion;
        }

        public static string GetExtensionVersion()
        {
            Assembly asm = Assembly.GetExecutingAssembly();
            string asmDir = Path.GetDirectoryName(asm.Location);
            string manifestPath = Path.Combine(asmDir, "extension.vsixmanifest");
            string version = null;
            if (File.Exists(manifestPath))
            {
                XmlDocument doc = new XmlDocument();
                doc.Load(manifestPath);
                XmlElement metaData = doc.DocumentElement.ChildNodes.Cast<XmlElement>().First(x => x.Name == "Metadata");
                XmlElement identity = metaData.ChildNodes.Cast<XmlElement>().First(x => x.Name == "Identity");
                version = identity.GetAttribute("Version");
            }
            return version;
        }

        public static string GetErrorPageInWebView(string errorMessage, string detailMessage, string remediationMessage)
        {
            return $@"<!DOCTYPE html>
            <html>
            <head>
                <title>Error</title>
                <style>
                    html {{
                        height: 100%;
                        width: 100%;
                        background-color: #2b2b2b;
                        color: #ffffff;
                        font-family: -apple-system, BlinkMacSystemFont, sans-serif;
                        font-size: 13px;
                        margin: 0;
                    }}
                    body {{
                        margin: 0;
                        height: 100%;
                        width: 100%;
                    }}
                    .empty-state {{
                        opacity: 0.7;
                        display: flex;
                        flex-direction: column;
                        align-items: center;
                        justify-content: center;
                        text-align: center;
                        letter-spacing: 2px;
                        color: white;
                        min-height: 100vh;
                        padding: 0 20px;
                    }}
                    .title {{
                              font-size: 20px;
                              font-weight: bold;
                              display: block;
                              padding-bottom: 10px;}}
                    .message {{
                        font-size: 12px;
                        display: block;
                        margin-top: 10px;
                        width:100%;
                        word-wrap: break-word;
                        padding: 0 10px;
                    }}
                </style>
            </head>
            <body>
                <div class='empty-state'>
                        <div class='title'>{errorMessage}</div>
                        <div class='message'>
                            {detailMessage}
                        </div>
                        <div class='message'>
                            {remediationMessage}
                        </div>
                </div>
            </body>
            </html>";
        }

        /// <summary>
        /// Logs the error, send it to scuba table and show error notification to users.
        /// </summary>
        public static async Task LogAndHandleErrorAsync(AsyncPackage package, ActionType action, ErrorCodes errorCode, String message, bool showNotification = true)
        {
            string errorMessage = $"Error code: {errorCode}, Message: {message}";

            // Log the error to the output window
            await LoggingHelper.WriteAsync(errorMessage);

            // Send telemetry for the error
            // @fb-only: telemetryLogger.logError(action, errorCode, message);

            // Show a message box to the user
            if (showNotification && package != null)
            {
                VsShellUtilities.ShowMessageBox(
                    package,
                    errorMessage,
                    "Error",
                    OLEMSGICON.OLEMSGICON_WARNING,
                    OLEMSGBUTTON.OLEMSGBUTTON_OK,
                    OLEMSGDEFBUTTON.OLEMSGDEFBUTTON_FIRST);
            }

            if (Constants.ErrorCodesThatShouldCloseToolWindow.Contains(errorCode) && package != null)
            {
                // Close the tool window
                await CloseToolWindowAsync(package);
            }

        }

        /// <summary>
        /// Logs successful actions to telemetry.
        /// </summary>
        /// <param name="action"></param>
        /// <returns></returns>
        public static async Task LogSuccessAsync(ActionType action)
        {
            // @fb-only: telemetryLogger.logInfo(action);
        }

        /// <summary>
        /// Closes the tool window.
        /// </summary>
        private static async Task CloseToolWindowAsync(AsyncPackage package)
        {
            await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync();
            var windowFrame = (IVsWindowFrame)package.FindToolWindow(typeof(InteractiveSmartlogToolWindow), 0, false)?.Frame;
            if (windowFrame != null)
            {
                windowFrame.CloseFrame((uint)__FRAMECLOSE.FRAMECLOSE_NoSave);
            }
        }
    }
}
