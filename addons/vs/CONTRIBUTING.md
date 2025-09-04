# Contributing to InteractiveSmartlogVSExtension

Thank you for your interest in contributing to the Interactive Smartlog Visual Studio Extension! This extension provides a tool window for Sapling users to perform source control operations (view commits, add changes, rebase, merge, etc.) and integrates with tools like Jellyfish and Phabricator for code review and merging.

Below are guidelines for building, running, testing, dogfooding, and debugging the extension.

---

## Table of Contents

- [Building & Running Locally](#building--running-locally)
- [Compiling for Local Debugging](#compiling-for-local-debugging)
- [Testing the Extension](#testing-the-extension)
- [Debugging](#debugging)
- [Additional Notes](#additional-notes)

---

## Building & Running Locally

1. **Prerequisites:**
   - Visual Studio 2022 (Community, Professional, or Enterprise)
   - .NET Framework 4.7.2 development tools
   - [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) runtime installed
   - A valid [Sapling](https://sapling-scm.com/) repository on your machine

2. **Clone the Repository:**
   git clone <repo-url>

3. **Open the Solution:**
   - Open `InteractiveSmartlogVSExtension.sln` in Visual Studio.

4. **Restore NuGet Packages:**
   - Visual Studio should automatically restore packages on open. If not, right-click the solution and select **Restore NuGet Packages**.

---

## Compiling for Local Debugging

1. **Set the Startup Project:**
   - Right-click the `InteractiveSmartlogVSExtension` project and select **Set as Startup Project**.

2. **Start Debugging:**
   - Press `F5` or select **Debug > Start Debugging**.
   - This will launch a new experimental instance of Visual Studio with the extension loaded.

3. **Using the Extension:**
   - Open a solution that is part of a valid Sapling repository.
   - Go to **View > Other Windows > Interactive Smartlog** to open the tool window.
   - Use **Tools > Reload ISL** to reload the view if needed.

---

## Testing the Extension

- **Manual Testing:**
  - Open the extension in an experimental instance as described above.
  - Perform source control operations (view commits, add changes, rebase, merge, etc.) in the Interactive Smartlog tool window.
  - Ensure the tool window loads correctly and interacts with your Sapling repository.

- **Repository Requirement:**
  - The extension requires a valid Sapling repository. Make sure to open a solution that is part of such a repository before launching the Interactive Smartlog view.

- **Reloading:**
  - Use **Tools > Reload ISL** to test the reload functionality.

---

## Debugging

- **Attach Debugger:**
  - Use `F5` to launch the experimental instance with debugging enabled.
  - Set breakpoints in your code as needed.

- **Logging:**
  - The extension uses logging (see `LoggingHelper`) to output diagnostic information. Check the "ISL for Visual Studio" Output window for logs.

- **Activity Log:**
  - Launch Visual Studio with the `/log` flag to generate an activity log: devenv.exe /log
  - Review `%APPDATA%\Microsoft\VisualStudio\<version>\ActivityLog.xml` for errors.

- **Common Issues:**
  - If the tool window does not load, ensure you have a valid Sapling repository and that the extension is enabled.
  - If the UI hangs, check for blocking calls or thread access issues in your code.

---

## Additional Notes

- **Tool Window Access:**
  - The extension adds a tool window accessible via **View > Other Windows > Interactive Smartlog**.

- **Reloading:**
  - The tool window can be reloaded via **Tools > Reload ISL**.

- **Contribution Guidelines:**
  - Please follow standard C# and Visual Studio extension development best practices.
  - Submit pull requests with clear descriptions and reference any related issues.

---

Thank you for contributing!
