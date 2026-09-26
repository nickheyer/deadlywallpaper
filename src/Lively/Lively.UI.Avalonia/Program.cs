using Avalonia;
using CommandLine;
using Lively.Common;
using Lively.UI.Avalonia.Services;
using NLog;
using NLog.Targets;
using System;
using System.IO;

namespace Lively.UI.Avalonia
{
    public static class Program
    {
        private static readonly Logger Logger = LogManager.GetCurrentClassLogger();

        [STAThread]
        public static int Main(string[] args)
        {
            ConfigureLogging();

            var startFlags = new StartArgs();
            Parser.Default.ParseArguments<StartArgs>(args)
                .WithParsed(x => startFlags = x)
                .WithNotParsed(errors =>
                {
                    foreach (var error in errors)
                        Logger.Error(error);
                });
            App.StartFlags = startFlags;

            using var instance = SingleInstanceGuard.Acquire();
            if (!instance.IsFirstInstance)
            {
                // Another UI process is running, hand it the show request and leave.
                instance.SendToRunningInstance("WM SHOW");
                return 0;
            }

            App.SingleInstance = instance;
            try
            {
                return BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
            }
            catch (Exception ex)
            {
                Logger.Fatal(ex);
                throw;
            }
            finally
            {
                LogManager.Shutdown();
            }
        }

        public static AppBuilder BuildAvaloniaApp()
            => AppBuilder.Configure<App>()
                .UsePlatformDetect()
                .WithInterFont()
                .LogToTrace();

        private static void ConfigureLogging()
        {
            Directory.CreateDirectory(Constants.CommonPaths.LogDirUI);
            var config = new NLog.Config.LoggingConfiguration();
            var fileTarget = new FileTarget("logfile")
            {
                FileName = Path.Combine(Constants.CommonPaths.LogDirUI, $"{DateTime.Now:yyyyMMdd_HHmmss}.txt"),
                MaxArchiveFiles = 4,
                ArchiveAboveSize = 50000000,
                Layout = "${longdate}|${level:uppercase=true}|${logger}|${message}${onexception:${newline}${exception:format=tostring}}",
            };
            var consoleTarget = new ConsoleTarget("logconsole")
            {
                Layout = "${level:uppercase=true}|${logger}|${message}${onexception:${newline}${exception:format=tostring}}",
                StdErr = true,
            };
            config.AddRule(LogLevel.Info, LogLevel.Fatal, consoleTarget);
            config.AddRule(LogLevel.Debug, LogLevel.Fatal, fileTarget);
            LogManager.Configuration = config;
        }
    }
}
