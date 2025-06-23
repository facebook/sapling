/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */


using System;
using System.ComponentModel.Design;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio.Shell.Interop;
using Task = System.Threading.Tasks.Task;
using Microsoft.VisualStudio.Threading;
using EnvDTE;
using EnvDTE80;
namespace InteractiveSmartlogVSExtension
{
    /// <summary>
    /// Command handler
    /// </summary>
    internal sealed class InteractiveSmartlogCommands
    {
        /// <summary>
        /// VS Package that provides this command, not null.
        /// </summary>
        private readonly AsyncPackage package;

        private InteractiveSmartlogVSExtensionPackage interactiveSmartlogVSExtensionPackage => package as InteractiveSmartlogVSExtensionPackage;

        /// <summary>
        /// Initializes a new instance of the <see cref="InteractiveSmartlogCommands"/> class.
        /// Adds our command handlers for menu (commands must exist in the command table file)
        /// </summary>
        /// <param name="package">Owner package, not null.</param>
        /// <param name="commandService">Command service to add command to, not null.</param>
        private InteractiveSmartlogCommands(AsyncPackage package, OleMenuCommandService commandService)
        {
            this.package = package ?? throw new ArgumentNullException(nameof(package));
            commandService = commandService ?? throw new ArgumentNullException(nameof(commandService));

            var menuCommandID = new CommandID(Constants.ToolsMenuCommandSet, Constants.ReloadCommandId);
            var menuItem = new MenuCommand(ReloadCommandHandler, menuCommandID);
            commandService.AddCommand(menuItem);

            menuCommandID = new CommandID(Constants.ContextMenuCommandSet, Constants.DiffUncommittedChangesCommandId);
            menuItem = new MenuCommand(DiffUncommittedChangesCommandHandler, menuCommandID);
            commandService.AddCommand(menuItem);

            menuCommandID = new CommandID(Constants.ContextMenuCommandSet, Constants.DiffStackChangesCommandId);
            menuItem = new MenuCommand(DiffStackChangesCommandHandler, menuCommandID);
            commandService.AddCommand(menuItem);

            menuCommandID = new CommandID(Constants.ContextMenuCommandSet, Constants.DiffHeadChangesCommandId);
            menuItem = new MenuCommand(DiffHeadChangesCommandHandler, menuCommandID);
            commandService.AddCommand(menuItem);

            menuCommandID = new CommandID(Constants.ContextMenuCommandSet, Constants.RevertUncommittedChangesCommandId);
            menuItem = new MenuCommand(RevertUncommittedChangesCommandHandler, menuCommandID);
            commandService.AddCommand(menuItem);
        }

        private void ReloadCommandHandler(object sender, EventArgs e)
        {
            package.JoinableTaskFactory.Run(async () => await ExecuteAsync(sender, e));
        }

        private void DiffUncommittedChangesCommandHandler(object sender, EventArgs e)
        {
            package.JoinableTaskFactory.Run(async () => await ExecuteAsync(sender, e));
        }

        private void DiffStackChangesCommandHandler(object sender, EventArgs e)
        {
            package.JoinableTaskFactory.Run(async () => await ExecuteAsync(sender, e));
        }

        private void DiffHeadChangesCommandHandler(object sender, EventArgs e)
        {
            package.JoinableTaskFactory.Run(async () => await ExecuteAsync(sender, e));
        }

        private void RevertUncommittedChangesCommandHandler(object sender, EventArgs e)
        {
            package.JoinableTaskFactory.Run(async () => await ExecuteAsync(sender, e));
        }

        /// <summary>
        /// Gets the instance of the command.
        /// </summary>
        public static InteractiveSmartlogCommands Instance
        {
            get;
            private set;
        }

        /// <summary>
        /// Initializes the singleton instance of the command.
        /// </summary>
        /// <param name="package">Owner package, not null.</param>
        public static async Task InitializeAsync(AsyncPackage package)
        {
            // Switch to the main thread - the call to AddCommand in InteractiveSmartlogCommands's constructor requires
            // the UI thread.
            await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync(package.DisposalToken);

            OleMenuCommandService commandService = await package.GetServiceAsync(typeof(IMenuCommandService)) as OleMenuCommandService;
            Instance = new InteractiveSmartlogCommands(package, commandService);
        }

        private async Task ExecuteAsync(object sender, EventArgs e)
        {
            try
            {
                var menuCommand = sender as MenuCommand;
                var menuCommandId = menuCommand.CommandID.ID;
                await ExecuteCommandAsync(menuCommandId);
            }
            catch (Exception ex)
            {
                VsShellUtilities.ShowMessageBox(
                    this.package,
                    ex.Message,
                    null,
                    OLEMSGICON.OLEMSGICON_WARNING,
                    OLEMSGBUTTON.OLEMSGBUTTON_OK,
                    OLEMSGDEFBUTTON.OLEMSGDEFBUTTON_FIRST);
            }
        }

        public async Task ExecuteCommandAsync(int commandId)
        {
            await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync(package.DisposalToken);
            DTE2 dte = Package.GetGlobalService(typeof(DTE)) as DTE2;
            if (dte == null)
            {
                throw new Exception("DTE not available");
            }

            Document activeDocument = dte.ActiveDocument;
            if (activeDocument == null)
                return;

            var path = activeDocument.FullName;
            if (String.IsNullOrEmpty(path))
                return;

            switch (commandId)
            {
                case Constants.DiffStackChangesCommandId:
                    await CommandHelper.ShowDiffAsync(interactiveSmartlogVSExtensionPackage, DiffType.STACK, activeDocument);
                    break;
                case Constants.DiffUncommittedChangesCommandId:
                    await CommandHelper.ShowDiffAsync(interactiveSmartlogVSExtensionPackage, DiffType.UNCOMMITTED, activeDocument);
                    break;
                case Constants.DiffHeadChangesCommandId:
                    await CommandHelper.ShowDiffAsync(interactiveSmartlogVSExtensionPackage, DiffType.HEAD, activeDocument);
                    break;
                case Constants.RevertUncommittedChangesCommandId:
                    const int yesButton = 6;
                    var result = VsShellUtilities.ShowMessageBox(
                        this.package,
                        $"Are you sure you want to revert {path}?",
                        "Revert Uncommitted Changes?",
                        OLEMSGICON.OLEMSGICON_QUERY,
                        OLEMSGBUTTON.OLEMSGBUTTON_YESNO,
                        OLEMSGDEFBUTTON.OLEMSGDEFBUTTON_SECOND);
                    if (result != yesButton)
                        return;
                    await CommandHelper.RevertChangesAsync(interactiveSmartlogVSExtensionPackage, path);
                    break;
                case Constants.ReloadCommandId:
                    await CommandHelper.ReloadISLToolWindowAsync(interactiveSmartlogVSExtensionPackage);
                    break;
                default:
                    throw new Exception($"Unrecognized command id: {commandId}");
            }
        }
    }
}
