using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Interactivity;
using Lively.Models.LivelyControls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.LivelyProperty
{
    /// <summary>
    /// Renders the wallpaper's LivelyProperties controls. Each control writes its value to its model and then
    /// reports the change to the view model command, which forwards it to the running wallpaper.
    /// </summary>
    public partial class LivelyPropertiesView : UserControl
    {
        private readonly CustomiseWallpaperViewModel viewModel;

        public LivelyPropertiesView(CustomiseWallpaperViewModel viewModel)
        {
            this.viewModel = viewModel;
            InitializeComponent();
            DataContext = viewModel;
        }

        private void Slider_ValueChanged(object sender, RangeBaseValueChangedEventArgs e)
        {
            if (sender is Slider slider && slider.DataContext is SliderModel model)
            {
                model.Value = slider.Value;
                viewModel.SliderValueChangedCommand.Execute(model);
            }
        }

        private void TextBox_TextChanged(object sender, TextChangedEventArgs e)
        {
            if (sender is TextBox textBox && textBox.DataContext is TextboxModel model)
            {
                model.Value = textBox.Text;
                viewModel.TextboxValueChangedCommand.Execute(model);
            }
        }

        private void ComboBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
        {
            if (sender is not ComboBox comboBox || comboBox.SelectedIndex < 0)
                return;

            switch (comboBox.DataContext)
            {
                case DropdownModel dropdown:
                    dropdown.Value = comboBox.SelectedIndex;
                    viewModel.DropdownValueChangedCommand.Execute(dropdown);
                    break;
                case ScalerDropdownModel scalerDropdown:
                    scalerDropdown.Value = comboBox.SelectedIndex;
                    viewModel.DropdownValueChangedCommand.Execute(scalerDropdown);
                    break;
            }
        }

        private void CheckBox_IsCheckedChanged(object sender, RoutedEventArgs e)
        {
            if (sender is CheckBox checkBox && checkBox.DataContext is CheckboxModel model)
            {
                model.Value = checkBox.IsChecked == true;
                viewModel.CheckboxValueChangedCommand.Execute(model);
            }
        }
    }
}
