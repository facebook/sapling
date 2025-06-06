/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using Microsoft.VisualStudio.Shell;
using System;
// @lint-ignore-every UNITYBANNEDAPI
using System.Runtime.InteropServices;
using System.Threading;
using Task = System.Threading.Tasks.Task;
using Microsoft.VisualStudio;
using Microsoft.VisualStudio.Shell.Interop;
using Microsoft.VisualStudio.ComponentModelHost;
using InteractiveSmartlogVSExtension.Models;
using InteractiveSmartlogVSExtension.Helpers;

namespace InteractiveSmartlogVSExtension
{
    /// <summary>
    /// This is the class that implements the package exposed by this assembly.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The minimum requirement for a class to be considered a valid package for Visual Studio
    /// is to implement the IVsPackage interface and register itself with the shell.
    /// This package uses the helper classes defined inside the Managed Package Framework (MPF)
    /// to do it: it derives from the Package class that provides the implementation of the
    /// IVsPackage interface and uses the registration attributes defined in the framework to
    /// register itself and its components with the shell. These attributes tell the pkgdef creation
    /// utility what data to put into .pkgdef file.
    /// </para>
    /// <para>
    /// To get loaded into VS, the package must be referred by &lt;Asset Type="Microsoft.VisualStudio.VsPackage" ...&gt; in .vsixmanifest file.
    /// </para>
    /// </remarks>

    [ProvideAutoLoad(UIContextGuids80.SolutionExists, PackageAutoLoadFlags.BackgroundLoad)]
    [ProvideAutoLoad(UIContextGuids80.NoSolution, PackageAutoLoadFlags.BackgroundLoad)]
    [PackageRegistration(UseManagedResourcesOnly = true, AllowsBackgroundLoading = true)]
    [Guid(InteractiveSmartlogVSExtensionPackage.PackageGuidString)]
    [ProvideMenuResource("Menus.ctmenu", 1)]
    [ProvideOptionPage(typeof(OptionsPage), "InteractiveSmartLog", "Options", 0, 0, true)]
    [ProvideToolWindow(typeof(InteractiveSmartlogToolWindow), Orientation = ToolWindowOrientation.Right, Window = EnvDTE.Constants.vsWindowKindSolutionExplorer,
           Style = VsDockStyle.Tabbed)]
    public sealed class InteractiveSmartlogVSExtensionPackage : AsyncPackage
    {
        /// <summary>
        /// InteractiveSmartlogVSExtensionPackage GUID string.
        /// </summary>
        public const string PackageGuidString = "ebb7f508-ce81-4d97-8805-ec46d95e7473";
        public static InteractiveSmartlogVSExtensionPackage Instance { get; private set; }
        public OptionsPage Options { get { return (OptionsPage)GetDialogPage(typeof(OptionsPage)); } }

        public DiffTool DiffTool { get { return Options == null ? DiffTool.VisualStudio : Options.DiffTool; } }
        public string CustomDiffToolExe { get { return Options?.CustomDiffToolExe; } }
        public string CustomDiffToolArgs { get { return Options?.CustomDiffToolArgs; } }

        #region Package Members

        /// <summary>
        /// Initialization of the package; this method is called right after the package is sited, so this is the place
        /// where you can put all the initialization code that rely on services provided by VisualStudio.
        /// </summary>
        /// <param name="cancellationToken">A cancellation token to monitor for initialization cancellation, which can occur when VS is shutting down.</param>
        /// <param name="progress">A provider for progress updates.</param>
        /// <returns>A task representing the async work of package initialization, or an already completed task if there is none. Do not return null from this method.</returns>
        protected override async Task InitializeAsync(CancellationToken cancellationToken, IProgress<ServiceProgressData> progress)
        {
            Instance = this;
            // When initialized asynchronously, the current thread may be a background thread at this point.
            // Do any initialization that requires the UI thread after switching to the UI thread.
            await this.JoinableTaskFactory.SwitchToMainThreadAsync(cancellationToken);

            Microsoft.VisualStudio.Shell.Events.SolutionEvents.OnAfterOpenSolution += OnSolutionOpened;
            Microsoft.VisualStudio.Shell.Events.SolutionEvents.OnAfterCloseSolution += OnSolutionClosed;

            await InteractiveSmartlogToolWindowCommand.InitializeAsync(this);
            await InteractiveSmartlogCommands.InitializeAsync(this);
        }

        private void OnSolutionOpened(object sender, EventArgs e)
        {
            ThreadHelper.JoinableTaskFactory.Run(async () =>
            {
                SmartlogUrlCache.LastComputedUrl = null;

                try
                {
                    SmartlogUrlCache.LastComputedUrl = await SmartlogUrlHelper.ComputeSlWebUrlStaticAsync();
                    SmartlogUrlCache.LastComputedTimestamp = DateTime.Now;
                }
                catch (Exception ex)
                {
                    SmartlogUrlCache.LastError = ex.Message;
                }
            });
        }

        private void OnSolutionClosed(object sender, EventArgs e)
        {
            // Clear the cached URL and related state
            SmartlogUrlCache.LastComputedUrl = null;
        }
        #endregion
    }
}
