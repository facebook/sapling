/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System.ComponentModel;
// @lint-ignore-every UNITYBANNEDAPI
using System.Drawing.Design;
using Microsoft.VisualStudio.Shell;

namespace InteractiveSmartlogVSExtension
{
    public class OptionsPage : DialogPage
    {
        [Category("Diff")]
        [DisplayName("Diff Tool")]
        [Description("Select your choice of diff tool.")]
        [TypeConverter(typeof(DiffToolConverter))]
        public DiffTool DiffTool { get; set; } = DiffTool.VisualStudio;

        [Category("Diff: Custom Diff Tool")]
        [DisplayName("Custom Diff Tool")]
        [Description("If using a custom diff tool, specify the executable path here.")]
        [Editor(typeof(FilePickerEditor), typeof(UITypeEditor))]
        public string CustomDiffToolExe { get; set; } = "";

        [Category("Diff: Custom Diff Tool")]
        [DisplayName("Custom Diff Tool Args")]
        [Description(
            "If using a custom diff tool, specify the arguments for it here.\n" +
            "Use the following variables for substitution:\n" +
            "  %bf : base filename\n" +
            "  %wf : working filename\n" +
            "  %bn : base file description\n" +
            "  %wn : working file description\n" +
            "Example for p4merge: -nl %bn -nr %wn %bf %wf"
            )]
        public string CustomDiffToolArgs { get; set; } = "";
    }
}
