/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.ComponentModel;
// @lint-ignore-every UNITYBANNEDAPI
using System.Drawing.Design;
using System.Windows.Forms;
using System.Windows.Forms.Design;

namespace InteractiveSmartlogVSExtension
{
    public class FilePickerEditor : UITypeEditor
    {
        private OpenFileDialog openFileDialog = new OpenFileDialog();
        public override UITypeEditorEditStyle GetEditStyle(ITypeDescriptorContext context)
        {
            return UITypeEditorEditStyle.Modal;
        }
        public override object EditValue(ITypeDescriptorContext context, IServiceProvider provider, object value)
        {
            if (provider == null)
            {
                return value;
            }

            IWindowsFormsEditorService editorService = provider.GetService(typeof(IWindowsFormsEditorService)) as IWindowsFormsEditorService;
            if (editorService != null)
            {
                if (openFileDialog.ShowDialog() == DialogResult.OK)
                {
                    value = openFileDialog.FileName;
                }
            }
            return value;
        }
    }
}
