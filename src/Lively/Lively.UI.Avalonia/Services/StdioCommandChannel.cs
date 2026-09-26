using Avalonia.Threading;
using System;
using System.IO;
using System.Threading;

namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// The command pipe to the wallpaper core: one command per stdin line ("WM SHOW", "LM SHOWBUSY", ...),
    /// one status line per stdout line ("LM UIVISIBLE true", "LM WALLPAPERDATA {...}").
    /// Nothing else may be written to stdout; logging goes to stderr and the log file.
    /// </summary>
    public sealed class StdioCommandChannel
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private readonly object writeGate = new object();
        private readonly TextWriter output = Console.Out;

        /// <summary>
        /// Raised on the UI thread for every non-blank line read from stdin.
        /// </summary>
        public event EventHandler<string> CommandReceived;

        /// <summary>
        /// Raised on the UI thread when stdin reaches end of file.
        /// </summary>
        public event EventHandler InputClosed;

        public void Start()
        {
            var thread = new Thread(ReadLoop)
            {
                IsBackground = true,
                Name = "stdin-commands",
            };
            thread.Start();
        }

        public void WriteLine(string line)
        {
            lock (writeGate)
            {
                output.WriteLine(line);
                output.Flush();
            }
        }

        private void ReadLoop()
        {
            try
            {
                string line;
                while ((line = Console.In.ReadLine()) != null)
                {
                    if (string.IsNullOrWhiteSpace(line))
                        continue;

                    var command = line;
                    Dispatcher.UIThread.Post(() => CommandReceived?.Invoke(this, command));
                }
            }
            catch (IOException ex)
            {
                Logger.Error(ex);
            }
            Dispatcher.UIThread.Post(() => InputClosed?.Invoke(this, EventArgs.Empty));
        }
    }
}
