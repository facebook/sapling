/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using EnvDTE;
using EnvDTE80;
using InteractiveSmartlogVSExtension.Enums;
using InteractiveSmartlogVSExtension.Helpers;
using InteractiveSmartlogVSExtension.Models;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio.Shell.Interop;
using Microsoft.VisualStudio.Threading;
using Microsoft.Web.WebView2.Core;
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Markup;

namespace InteractiveSmartlogVSExtension
{
    /// <summary>
    /// Interaction logic for InteractiveSmartlogToolWindowControl.
    /// </summary>
    public partial class InteractiveSmartlogToolWindowControl : System.Windows.Controls.UserControl, IComponentConnector
    {

        private AsyncPackage _package;

        /// <summary>
        /// Initializes a new instance of the <see cref="InteractiveSmartlogToolWindowControl"/> class.
        /// </summary>
        public InteractiveSmartlogToolWindowControl()
        {
            this.InitializeComponent();

            // Initialize the package instance asynchronously
            ThreadHelper.JoinableTaskFactory.Run(InitializePackageAsync);

            progressBar.Visibility = Visibility.Visible; // Show progressBar initially

            this.Loaded += OnLoaded;
        }

        private async Task InitializePackageAsync()
        {
            int attempts = 0;

            while (InteractiveSmartlogVSExtensionPackage.Instance == null && attempts < Constants.MaxToolWindowInitializationAttempts)
            {
                // Calculate the delay with exponential backoff
                int delay = Constants.ToolWindowInitializationRetryDelayMs * (int)Math.Pow(2, attempts);

                // Asynchronously wait for the package to initialize
                await Task.Delay(delay);
                attempts++;
            }

            if (InteractiveSmartlogVSExtensionPackage.Instance == null)
            {
                await CommonHelper.LogAndHandleErrorAsync(null, ActionType.RenderISLView, ErrorCodes.PackageInitializationFailed, "Instance is not initialized");
            }

            _package = InteractiveSmartlogVSExtensionPackage.Instance;
        }

        // @lint-ignore-every UNITYBANNEDAPI - This is not Unity code but a VSIX project

        private static TaskCompletionSource<bool> _webViewLoadedTcs = new TaskCompletionSource<bool>();

        /// <summary>
        /// Handles the Loaded event of the UserControl.
        /// </summary>
        /// <param name="sender"></param>
        /// <param name="args"></param>
        private async void OnLoaded(object sender, RoutedEventArgs args)
        {
            this.Loaded -= OnLoaded;
            await InitializeWebViewAsync();
            await LoadWebViewWithCachedUrlAsync();
        }

        public static Task WaitForWebViewToLoadAsync()
        {
            // Use JoinableTaskFactory.RunAsync to start the task within the current context
            return ThreadHelper.JoinableTaskFactory.RunAsync(async () =>
            {
                await _webViewLoadedTcs.Task.ConfigureAwait(false);
            }).Task;
        }

        /// <summary>
        /// Reloads the webview.
        /// </summary>
        public async Task ReloadAsync()
        {

            await ExecuteCommandAndLoadUrlAsync();

        }


        /// <summary>
        /// Renders url within a webview.
        /// </summary>
        private async Task InitializeWebViewAsync()
        {
            // Create webview directory
            string webviewDirectory = await CreateWebViewDirectoryAsync();
            // Create webview2 environment
            CoreWebView2Environment env = await CreateWebView2EnvironmentAsync(webviewDirectory);
            // Load the webview2
            await LoadWebView2Async(env);
        }

        /// <summary>
        /// Creates the webview directory asynchronously.
        /// </summary>
        /// <returns>Webview directory path.</returns>
        private Task<string> CreateWebViewDirectoryAsync()
        {
            try
            {
                string webviewDirectory = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
                Directory.CreateDirectory(webviewDirectory);
                return Task.FromResult(webviewDirectory);
            }
            catch (Exception ex)
            {
                CommonHelper.LogAndHandleErrorAsync(_package, ActionType.RenderISLView, ErrorCodes.WebViewDirectoryCreationFailed, ex.Message);
                return null;
            }
        }

        /// <summary>
        /// Creates the webview2 environment asynchronously.
        /// </summary>
        /// <param name="webviewDirectory">Webview directory path.</param>
        /// <returns>Webview2 environment.</returns>
        async Task<CoreWebView2Environment> CreateWebView2EnvironmentAsync(string webviewDirectory)
        {
            try
            {
                return await CoreWebView2Environment.CreateAsync(null, webviewDirectory);
            }
            catch (Exception ex)
            {
                await CommonHelper.LogAndHandleErrorAsync(_package, ActionType.RenderISLView, ErrorCodes.WebViewEnvironmentCreationFailed, ex.Message);
                return null;
            }
        }

        /// <summary>
        /// Loads the webview2 asynchronously.
        /// </summary>
        /// <param name="env">Webview2 environment.</param>
        async Task LoadWebView2Async(CoreWebView2Environment env)
        {
            await webView.EnsureCoreWebView2Async(env);
            if (webView == null)
            {
                await CommonHelper.LogAndHandleErrorAsync(_package, ActionType.RenderISLView, ErrorCodes.WebViewInitializationFailed, "The WebView control is not initialized.");
            }

            if (webView.CoreWebView2 == null)
            {
                await CommonHelper.LogAndHandleErrorAsync(_package, ActionType.RenderISLView, ErrorCodes.WebViewInitializationFailed, "CoreWebView2 is not initialized.");
            }

            SetupVSIdeBridge();
            webView.CoreWebView2.NewWindowRequested += CoreWebView2_NewWindowRequested;
            webView.CoreWebView2.NavigationCompleted += WebView_NavigationCompleted;
        }

        /// <summary>
        /// Handles the NewWindowRequested event.
        /// </summary>
        /// <param name="sender"></param>
        /// <param name="e"></param>
        private void CoreWebView2_NewWindowRequested(object sender, CoreWebView2NewWindowRequestedEventArgs e)
        {
            // Open the URL in the default browser instead of WebView (Edge)
            e.Handled = true;
            System.Diagnostics.Process.Start(new ProcessStartInfo(e.Uri) { UseShellExecute = true });
        }

        /// <summary>
        /// Handles the NavigationCompleted event.
        /// </summary>
        /// <param name="sender"></param>
        /// <param name="e"></param>
        private void WebView_NavigationCompleted(object sender, CoreWebView2NavigationCompletedEventArgs e)
        {
            progressBar.Visibility = Visibility.Collapsed;
            // Notify that the WebView is loaded
            _webViewLoadedTcs.TrySetResult(true);
        }

        /// <summary>
        /// Always recompute the URL (ignores the cache), then update the cache and load the new URL.
        /// Forcing a fresh computation, such as when the user explicitly reloads or when you know the cache may be stale.
        /// Ensures the latest state is always reflected, regardless of cache.
        /// </summary>
        /// <returns></returns>
        public async Task ExecuteCommandAndLoadUrlAsync()
        {
            try
            {
                var cancellationTokenSource = new CancellationTokenSource(TimeSpan.FromMilliseconds(Constants.OverallSlOperationTimeoutMs));
                string url = await RetryHelper.RetryWithExponentialBackoffAsync(
                    () => SmartlogUrlHelper.ComputeSlWebUrlStaticAsync(cancellationTokenSource.Token),
                    maxAttempts: Constants.MaxSlOperationRetries,
                    initialDelayMs: Constants.SlOperationRetryDelayMs
                );
                SmartlogUrlCache.LastComputedUrl = url;
                SmartlogUrlCache.LastComputedTimestamp = DateTime.Now;

                await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync();
                webView.Source = new Uri(url);
                await CommonHelper.LogSuccessAsync(ActionType.RenderISLView);
                await LoggingHelper.WriteAsync("Computed URL: " + url + " at " + DateTime.Now);
            }
            catch (FileNotFoundException ex) when (ex.Message.Contains("sl.exe"))
            {
                // Show a custom error page with a link to Sapling installation docs
                await DisplayErrorPageAsync(Constants.SaplingExeNotFoundTitle, Constants.SaplingExeNotFoundDetail, Constants.SaplingExeNotFoundRemediation);
                await CommonHelper.LogAndHandleErrorAsync(_package, ActionType.RenderISLView, ErrorCodes.SlWebFailed, ex.Message, false);
                return;

            }
            catch (Exception ex)
            {

                await CommonHelper.LogAndHandleErrorAsync(_package, ActionType.RenderISLView, ErrorCodes.SlWebFailed, ex.Message, false);
                // Try to extract the error output for more robust matching
                string message = ex.Message ?? string.Empty;

                // Check for exit code 255 and specific error messages
                if (message.Contains("ExitCode: 255"))
                {
                    if (message.IndexOf("is not inside a repository", StringComparison.OrdinalIgnoreCase) >= 0)
                    {
                        await DisplayErrorPageAsync(Constants.NoRepoErrorTitle, Constants.NoRepoErrorDetail, Constants.NoRepoErrorRemediation);
                        return;
                    }
                    else if (message.IndexOf("repository is not mounted", StringComparison.OrdinalIgnoreCase) >= 0)
                    {
                        // @fb-only: await DisplayErrorPageAsync(Constants.NoRepoMountedErrorTitle, Constants.NoRepoMountedErrorDetail, Constants.NoRepoMountedErrorRemediation);
                        return;
                    }
                }
                // Fallback: generic error page
                await DisplayErrorPageAsync(
                    "Failed to load Interactive Smartlog",
                    message,
                    "Try reloading the tool window via Tools -> Reload ISL"
                );
            }
        }

        /// <summary>
        /// Use the cached URL if available and valid.
        /// If not cached: Compute the URL, cache it, and then load it.
        /// Efficiently load the Smartlog view when the tool window is opened, minimizing redundant computation.
        /// </summary>
        /// <returns></returns>
        public async Task LoadWebViewWithCachedUrlAsync()
        {
            string url = SmartlogUrlCache.LastComputedUrl;
            // If the cached URL is not stale, use it; otherwise recompute
            if (!string.IsNullOrEmpty(url) && Uri.IsWellFormedUriString(url, UriKind.Absolute) && SmartlogUrlHelper.IsCachedUrlFresh(SmartlogUrlCache.LastComputedTimestamp))
            {
                await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync();
                webView.Source = new Uri(url);
            }
            else
            {
                await ExecuteCommandAndLoadUrlAsync();
            }
        }

        /// <summary>
        /// Displays an error page in the webview.
        /// </summary>
        /// <param name="title"></param>
        /// <param name="detail"></param>
        /// <param name="remediation"></param>
        /// <returns></returns>
        private async Task DisplayErrorPageAsync(string title, string detail, string remediation)
        {
            string htmlContent = CommonHelper.GetErrorPageInWebView(title, detail, remediation);
            await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync();
            webView.Source = new Uri($"data:text/html;charset=utf-8,{Uri.EscapeDataString(htmlContent)}");
        }

        /// <summary>
        /// Sets up the JavaScript bridge for communication between the webview and Visual Studio.
        /// </summary>
        private void SetupVSIdeBridge()
        {
            webView.CoreWebView2.WebMessageReceived += WebView_WebMessageReceived;

            _ = webView.CoreWebView2.AddScriptToExecuteOnDocumentCreatedAsync(@"
        window.__vsIdeBridge = {
            openFileInVisualStudio: (filePath, line=1, col=1) => {
                let data = JSON.stringify({ filePath, line, col });
                let message = JSON.stringify({ type: 'openFile', data: data });
                window.chrome.webview.postMessage(message);
            },
            showMessage: (message) => {
                window.chrome.webview.postMessage({ type: 'showMessage', data: message });
            },
            logMessage: (message) => {
                window.chrome.webview.postMessage({ type: 'logMessage', data: message });
            },
            openDiffInVisualStudio: (filePath, comparison) => {
                let data = JSON.stringify({ filePath, comparison });
                let message = JSON.stringify({ type: 'openDiff', data: data });
                window.chrome.webview.postMessage(message);
            }
        };
    ");
        }

        /// <summary>
        /// Handles the WebMessageReceived event.
        /// </summary>
        /// <param name="sender"></param>
        /// <param name="e"></param>
        private void WebView_WebMessageReceived(object sender, CoreWebView2WebMessageReceivedEventArgs e)
        {
            ThreadHelper.JoinableTaskFactory.Run(async () =>
            {
                try
                {
                    await WebView_WebMessageReceivedAsync(sender, e);
                }
                catch (Exception ex)
                {
                    await LoggingHelper.WriteAsync($"Error in WebMessageReceived event handler: {ex.Message}");
                    throw ex;
                }
            });
        }

        /// <summary>
        /// Handles the WebMessageReceived event asynchronously.
        /// </summary>
        /// <param name="sender"></param>
        /// <param name="e"></param>
        /// <returns></returns>
        /// <exception cref="InvalidOperationException"></exception>
        private async Task WebView_WebMessageReceivedAsync(object sender, CoreWebView2WebMessageReceivedEventArgs e)
        {
            await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync();
            try
            {
                var message = e.TryGetWebMessageAsString();
                if (message != null)
                {
                    var json = System.Text.Json.JsonDocument.Parse(message);
                    var type = json.RootElement.GetProperty("type").GetString();
                    var data = json.RootElement.GetProperty("data").GetString();

                    switch (type)
                    {
                        case "openFile":
                        {
                            var fileLocation = System.Text.Json.JsonSerializer.Deserialize<FileLocation>(data);
                            await OpenFileInVisualStudioAsync(fileLocation);
                            break;
                        }
                        case "showMessage":
                            LoggingHelper.ShowMessage(data);
                            break;
                        case "logMessage":
                            _ = LoggingHelper.WriteAsync(data);
                            break;
                        case "openDiff":
                        {
                            var diffData = System.Text.Json.JsonSerializer.Deserialize<DiffData>(data);
                            await OpenDiffViewInVisualStudioAsync(diffData);
                            break;
                        }
                    }
                }
            }
            catch (InvalidCastException ex)
            {
                if (ex.HResult == -2147467262)
                {
                    string error = $"CoreWebView2 members can only be accessed from the UI thread. {ex}";
                    await LoggingHelper.WriteAsync(error);
                    throw new InvalidOperationException(error);
                }
                throw;
            }
            catch (COMException ex2)
            {
                if (ex2.HResult == -2147019873)
                {
                    string error = $"CoreWebView2 members cannot be accessed after the WebView2 control is disposed. {ex2}";
                    await LoggingHelper.WriteAsync(error);
                    throw new InvalidOperationException(error);
                }
                throw;
            }
            catch (Exception ex)
            {
                await LoggingHelper.WriteAsync($"Error in WebView_WebMessageReceivedAsync: {ex.Message}");
                throw ex;
            }
        }

        /// <summary>
        /// Opens a file in Visual Studio at the specified line and column.
        /// </summary>
        /// <param name="fileLocation"></param>
        private async Task OpenFileInVisualStudioAsync(FileLocation fileLocation)
        {
            try
            {
                if (string.IsNullOrEmpty(fileLocation.FilePath) || fileLocation.Line < 1 || fileLocation.Col < 0)
                {
                    throw new ArgumentException($"Invalid file location data: {fileLocation.FilePath}");
                }

                if (!File.Exists(fileLocation.FilePath))
                {
                    throw new FileNotFoundException($"File not found: {fileLocation.FilePath}");
                }

                await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync();
                var dte = (DTE)Package.GetGlobalService(typeof(DTE));
                if (dte != null)
                {
                    await LoggingHelper.WriteAsync("Trying to open file: " + fileLocation.FilePath + " at line: " + fileLocation.Line + " and column: " + fileLocation.Col);
                    var window = dte.ItemOperations.OpenFile(fileLocation.FilePath);
                    var textSelection = (EnvDTE.TextSelection)dte.ActiveDocument.Selection;

                    if (fileLocation.Line > 0 && fileLocation.Col >= 0)
                    {
                        textSelection.MoveToLineAndOffset(fileLocation.Line, fileLocation.Col);

                    }
                    else
                    {
                        throw new ArgumentException("Line and column values must be greater than 0");
                    }
                    await CommonHelper.LogSuccessAsync(ActionType.OpenFile);
                }
            }
            catch (ArgumentException ex)
            {
                await CommonHelper.LogAndHandleErrorAsync(_package, ActionType.OpenFile, ErrorCodes.InvalidFileLocation, $"Invalid file location: {fileLocation.FilePath}: {ex.Message}");
            }
            catch (FileNotFoundException ex)
            {
                await CommonHelper.LogAndHandleErrorAsync(_package, ActionType.OpenFile, ErrorCodes.FileNotFound, $"File not found: {fileLocation.FilePath}: {ex.Message}");
            }
            catch (Exception ex)
            {
                await CommonHelper.LogAndHandleErrorAsync(_package, ActionType.OpenFile, ErrorCodes.FileOpenFailed, $"Failed to open file: {fileLocation.FilePath}: {ex.Message}");
            }
        }

        /// <summary>
        /// openDiff command handler
        /// </summary>
        /// <param name="diffData"></param>
        private async Task OpenDiffViewInVisualStudioAsync(DiffData diffData)
        {
            try
            {
                if (diffData == null || string.IsNullOrEmpty(diffData.FilePath) || !File.Exists(diffData.FilePath))
                {
                    throw new ArgumentException($"Invalid diff data: {diffData}");
                }

                await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync();

                var smartlogPackage = _package as InteractiveSmartlogVSExtensionPackage;

                await LoggingHelper.WriteAsync("Trying to render diff view");

                // Render diff view
                await CommandHelper.ShowInternalDiffAsync(smartlogPackage, DiffType.HEAD, diffData.FilePath, false);

                // Log success
                await CommonHelper.LogSuccessAsync(ActionType.OpenInternalDiffView);
            }
            catch (FileNotFoundException ex)
            {
                await CommonHelper.LogAndHandleErrorAsync(_package, ActionType.OpenInternalDiffView, ErrorCodes.FileNotFound, $"File not found: {ex.Message}");
            }
            catch (Exception ex)
            {
                await CommonHelper.LogAndHandleErrorAsync(_package, ActionType.OpenInternalDiffView, ErrorCodes.DiffViewRenderingFailed, $"{ex.Message}");
            }
        }
    }
}
