using Lively.Common.Services;
using Newtonsoft.Json;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Services
{
    /// <summary>
    /// Data the user entered in the add/edit wallpaper dialog shown by the UI process.
    /// </summary>
    public sealed class WallpaperDataResult
    {
        [JsonProperty("ok")] public bool Ok { get; set; }
        [JsonProperty("title")] public string Title { get; set; }
        [JsonProperty("author")] public string Author { get; set; }
        [JsonProperty("desc")] public string Desc { get; set; }
        [JsonProperty("contact")] public string Contact { get; set; }
    }

    /// <summary>
    /// Starts and talks to the desktop UI process (Lively.UI.Avalonia). Commands go to its stdin
    /// (same "WM"/"LM" lines as the Windows UI), status and dialog answers come back on its stdout.
    /// </summary>
    public sealed class LinuxRunnerService : IRunnerService
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly object sync = new object();
        private readonly string fileName;
        private readonly List<string> baseArguments = new List<string>();
        private Process processUI;
        private bool isVisible;
        private bool disposed;
        private TaskCompletionSource<WallpaperDataResult> pendingWallpaperData;

        public IntPtr HwndUI => IntPtr.Zero;
        public bool IsVisibleUI => processUI != null && isVisible;

        public LinuxRunnerService()
        {
            var command = Environment.GetEnvironmentVariable("LIVELY_UI_COMMAND");
            if (string.IsNullOrWhiteSpace(command))
            {
                fileName = Path.Combine(AppContext.BaseDirectory, "plugins", "UI", "Lively.UI.Avalonia");
            }
            else
            {
                var parts = SplitCommand(command);
                fileName = parts[0];
                baseArguments.AddRange(parts.GetRange(1, parts.Count - 1));
            }
        }

        public void ShowUI() => ShowUI(null, "WM SHOW");

        public void ShowAppUpdatePage() => ShowUI("--appUpdate true", "LM SHOWAPPUPDATEPAGE");

        public void ShowCustomisWallpaperePanel()
        {
            if (processUI == null)
                ShowUI("--trayWidget true", null);
            else
                Send("LM SHOWCUSTOMISEPANEL");
        }

        public void RestartUI(string startArgs = null)
        {
            Process old;
            lock (sync)
            {
                old = processUI;
                processUI = null;
            }
            if (old != null)
            {
                try
                {
                    old.Exited -= Proc_UI_Exited;
                    old.OutputDataReceived -= Proc_OutputDataReceived;
                    old.StandardInput.WriteLine("WM QUIT");
                    old.StandardInput.Flush();
                    if (!old.WaitForExit(2000))
                        old.Kill();
                    old.Dispose();
                }
                catch (Exception e)
                {
                    Logger.Error(e);
                }
            }
            ShowUI(startArgs, null);
        }

        public void CloseUI()
        {
            if (processUI == null)
                return;
            Send("WM HIDE");
        }

        public void SaveRectUI()
        {
            // Wayland compositors own window placement; there is no rect to save.
        }

        public void SetBusyUI(bool isBusy) => Send(isBusy ? "LM SHOWBUSY" : "LM HIDEBUSY");

        /// <summary>
        /// Asks the UI to show the wallpaper metadata dialog and waits for the answer.
        /// </summary>
        public async Task<WallpaperDataResult> RequestWallpaperDataAsync(string infoPath, string title, string author, string desc, string contact, string thumbnailPath, TimeSpan timeout)
        {
            var tcs = new TaskCompletionSource<WallpaperDataResult>(TaskCreationOptions.RunContinuationsAsynchronously);
            lock (sync)
            {
                if (pendingWallpaperData != null && !pendingWallpaperData.Task.IsCompleted)
                    throw new InvalidOperationException("A wallpaper dialog is already open.");
                pendingWallpaperData = tcs;
            }

            var payload = JsonConvert.SerializeObject(new
            {
                infoPath,
                title = title ?? string.Empty,
                author = author ?? string.Empty,
                desc = desc ?? string.Empty,
                contact = contact ?? string.Empty,
                thumbnail = thumbnailPath ?? string.Empty,
            });
            ShowUI(null, "LM WALLPAPERDATA " + payload);

            using var cts = new CancellationTokenSource(timeout);
            using (cts.Token.Register(() => tcs.TrySetException(new TimeoutException("The UI did not answer the wallpaper dialog."))))
            {
                return await tcs.Task;
            }
        }

        private void ShowUI(string startArgs, string command)
        {
            lock (sync)
            {
                if (processUI != null)
                {
                    if (command != null)
                        Send(command);
                    return;
                }

                try
                {
                    var process = new Process
                    {
                        StartInfo = new ProcessStartInfo
                        {
                            FileName = fileName,
                            RedirectStandardInput = true,
                            RedirectStandardOutput = true,
                            RedirectStandardError = false,
                            UseShellExecute = false,
                            WorkingDirectory = Path.GetDirectoryName(fileName) ?? AppContext.BaseDirectory,
                            StandardInputEncoding = new UTF8Encoding(false),
                            StandardOutputEncoding = Encoding.UTF8,
                        },
                        EnableRaisingEvents = true
                    };
                    foreach (var arg in baseArguments)
                        process.StartInfo.ArgumentList.Add(arg);
                    process.StartInfo.ArgumentList.Add("--core-managed");
                    if (!string.IsNullOrWhiteSpace(startArgs))
                        foreach (var arg in SplitCommand(startArgs))
                            process.StartInfo.ArgumentList.Add(arg);

                    process.Exited += Proc_UI_Exited;
                    process.OutputDataReceived += Proc_OutputDataReceived;
                    process.Start();
                    process.BeginOutputReadLine();
                    processUI = process;
                    isVisible = true;
                    Logger.Info($"UI started: {fileName} {string.Join(" ", process.StartInfo.ArgumentList)}");
                    if (command != null && command.StartsWith("LM WALLPAPERDATA", StringComparison.Ordinal))
                        Send(command);
                }
                catch (Exception e)
                {
                    Logger.Error(e);
                    processUI = null;
                    pendingWallpaperData?.TrySetException(new InvalidOperationException($"Failed to start the UI ({fileName}): {e.Message}", e));
                }
            }
        }

        private void Send(string line)
        {
            var p = processUI;
            if (p == null)
                return;
            try
            {
                p.StandardInput.WriteLine(line);
                p.StandardInput.Flush();
            }
            catch (Exception e)
            {
                Logger.Error($"UI stdin write failed: {e.Message}");
            }
        }

        private void Proc_OutputDataReceived(object sender, DataReceivedEventArgs e)
        {
            if (string.IsNullOrEmpty(e.Data))
                return;

            const string visiblePrefix = "LM UIVISIBLE ";
            const string dataPrefix = "LM WALLPAPERDATA ";
            if (e.Data.StartsWith(visiblePrefix, StringComparison.Ordinal))
            {
                isVisible = e.Data.Substring(visiblePrefix.Length).Trim().Equals("true", StringComparison.OrdinalIgnoreCase);
            }
            else if (e.Data.StartsWith(dataPrefix, StringComparison.Ordinal))
            {
                TaskCompletionSource<WallpaperDataResult> tcs;
                lock (sync)
                {
                    tcs = pendingWallpaperData;
                    pendingWallpaperData = null;
                }
                try
                {
                    var result = JsonConvert.DeserializeObject<WallpaperDataResult>(e.Data.Substring(dataPrefix.Length));
                    tcs?.TrySetResult(result ?? new WallpaperDataResult { Ok = false });
                }
                catch (JsonException ex)
                {
                    tcs?.TrySetException(new InvalidOperationException($"UI sent an invalid wallpaper dialog answer: {ex.Message}"));
                }
            }
            else
            {
                Logger.Info($"UI: {e.Data}");
            }
        }

        private void Proc_UI_Exited(object sender, EventArgs e)
        {
            lock (sync)
            {
                if (processUI == sender)
                {
                    processUI.Dispose();
                    processUI = null;
                    isVisible = false;
                }
                pendingWallpaperData?.TrySetResult(new WallpaperDataResult { Ok = false });
                pendingWallpaperData = null;
            }
            Logger.Info("UI process exited.");
        }

        private static List<string> SplitCommand(string command)
        {
            var result = new List<string>();
            var current = new StringBuilder();
            var inQuotes = false;
            foreach (var ch in command)
            {
                if (ch == '"') { inQuotes = !inQuotes; continue; }
                if (char.IsWhiteSpace(ch) && !inQuotes)
                {
                    if (current.Length > 0) { result.Add(current.ToString()); current.Clear(); }
                    continue;
                }
                current.Append(ch);
            }
            if (current.Length > 0)
                result.Add(current.ToString());
            return result;
        }

        public void Dispose()
        {
            if (disposed) return;
            disposed = true;
            Process p;
            lock (sync) { p = processUI; processUI = null; }
            if (p == null) return;
            try
            {
                p.StandardInput.WriteLine("WM QUIT");
                p.StandardInput.Flush();
                if (!p.WaitForExit(2000))
                    p.Kill();
            }
            catch (Exception ex)
            {
                Logger.Warn($"Stopping UI: {ex.Message}");
            }
            finally
            {
                p.Dispose();
            }
        }
    }
}
