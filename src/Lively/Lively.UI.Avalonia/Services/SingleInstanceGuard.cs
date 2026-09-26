using System;
using System.IO;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// Ensures only one UI process runs per user by owning a Unix domain socket. A second process connects to the
    /// socket, sends a command line (same syntax as the stdin commands) and exits.
    /// </summary>
    public sealed class SingleInstanceGuard : IDisposable
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly Socket listener;
        private readonly CancellationTokenSource cts = new CancellationTokenSource();
        private bool disposed;

        private SingleInstanceGuard(Socket listener, bool isFirstInstance)
        {
            this.listener = listener;
            IsFirstInstance = isFirstInstance;
        }

        public bool IsFirstInstance { get; }

        /// <summary>
        /// Raised on a thread-pool thread with each line received from another instance.
        /// </summary>
        public event EventHandler<string> CommandReceived;

        public static string SocketPath
        {
            get
            {
                var runtimeDir = Environment.GetEnvironmentVariable("XDG_RUNTIME_DIR");
                var baseDir = !string.IsNullOrEmpty(runtimeDir) && Directory.Exists(runtimeDir) ? runtimeDir : Path.GetTempPath();
                return Path.Combine(baseDir, $"lively-ui-{Environment.UserName}.sock");
            }
        }

        public static SingleInstanceGuard Acquire()
        {
            var path = SocketPath;
            var endpoint = new UnixDomainSocketEndPoint(path);

            if (File.Exists(path))
            {
                // Either another instance owns the socket or it is stale from a crashed process.
                using var probe = new Socket(AddressFamily.Unix, SocketType.Stream, ProtocolType.Unspecified);
                try
                {
                    probe.Connect(endpoint);
                    return new SingleInstanceGuard(null, false);
                }
                catch (SocketException)
                {
                    File.Delete(path);
                }
            }

            var listener = new Socket(AddressFamily.Unix, SocketType.Stream, ProtocolType.Unspecified);
            listener.Bind(endpoint);
            listener.Listen(4);
            var guard = new SingleInstanceGuard(listener, true);
            _ = guard.AcceptLoopAsync();
            return guard;
        }

        public void SendToRunningInstance(string command)
        {
            using var client = new Socket(AddressFamily.Unix, SocketType.Stream, ProtocolType.Unspecified);
            client.Connect(new UnixDomainSocketEndPoint(SocketPath));
            client.Send(Encoding.UTF8.GetBytes(command + "\n"));
        }

        private async Task AcceptLoopAsync()
        {
            while (!cts.IsCancellationRequested)
            {
                Socket client;
                try
                {
                    client = await listener.AcceptAsync(cts.Token);
                }
                catch (OperationCanceledException)
                {
                    break;
                }
                catch (ObjectDisposedException)
                {
                    break;
                }
                catch (SocketException ex)
                {
                    Logger.Error(ex);
                    continue;
                }

                _ = ReadClientAsync(client);
            }
        }

        private async Task ReadClientAsync(Socket client)
        {
            try
            {
                using (client)
                using (var stream = new NetworkStream(client, ownsSocket: false))
                using (var reader = new StreamReader(stream, Encoding.UTF8))
                {
                    string line;
                    while ((line = await reader.ReadLineAsync(cts.Token)) != null)
                    {
                        if (!string.IsNullOrWhiteSpace(line))
                            CommandReceived?.Invoke(this, line.Trim());
                    }
                }
            }
            catch (OperationCanceledException)
            {
            }
            catch (Exception ex)
            {
                Logger.Error(ex);
            }
        }

        public void Dispose()
        {
            if (disposed)
                return;
            disposed = true;

            cts.Cancel();
            if (listener != null)
            {
                try
                {
                    listener.Close();
                }
                catch { }
                try
                {
                    File.Delete(SocketPath);
                }
                catch { }
            }
            cts.Dispose();
        }
    }
}
