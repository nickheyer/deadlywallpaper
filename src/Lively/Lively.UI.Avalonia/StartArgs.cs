using CommandLine;

namespace Lively.UI.Avalonia
{
    public class StartArgs
    {
        [Option("showApp",
        Required = false,
        HelpText = "Show the main window on start.")]
        public bool ShowApp { get; set; }

        [Option("trayWidget",
        Required = false,
        HelpText = "Run customise-traymenu without initializing MainWindow.")]
        public bool TrayWidget { get; set; }

        [Option("appUpdate",
        Required = false,
        HelpText = "Open update page.")]
        public bool AppUpdate { get; set; }

        [Option("core-managed",
        Required = false,
        HelpText = "The core launched this process and re-shows it with 'WM SHOW'; closing the window hides it instead of exiting.")]
        public bool CoreManaged { get; set; }
    }
}
