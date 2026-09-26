using Lively.Common;
using System;
using System.IO;
using System.IO.Pipes;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Services
{
    public sealed class CoreConnectionStatus
    {
        public CoreConnectionStatus(bool isAvailable, string details)
        {
            IsAvailable = isAvailable;
            Details = details;
        }

        public bool IsAvailable { get; }

        /// <summary>
        /// Human readable reason when the core is unavailable, including the socket path that was probed.
        /// </summary>
        public string Details { get; }
    }

    /// <summary>
    /// Checks whether the wallpaper core is accepting gRPC connections on its named pipe.
    /// On Linux the .NET named pipe is a Unix domain socket in the temp directory.
    /// </summary>
    public static class CoreConnectionProbe
    {
        public static string PipeName => Constants.SingleInstance.GrpcPipeServerName;

        /// <summary>
        /// Path of the Unix domain socket that <see cref="NamedPipeClientStream"/> connects to for <see cref="PipeName"/>.
        /// </summary>
        public static string SocketPath => Path.Combine(Path.GetTempPath(), "CoreFxPipe_" + PipeName);

        public static async Task<CoreConnectionStatus> ProbeAsync(TimeSpan timeout, CancellationToken cancellationToken = default)
        {
            if (!File.Exists(SocketPath))
                return new CoreConnectionStatus(false, $"No core socket at {SocketPath}.");

            try
            {
                using var pipe = new NamedPipeClientStream(".", PipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
                await pipe.ConnectAsync((int)timeout.TotalMilliseconds, cancellationToken);
                return new CoreConnectionStatus(true, string.Empty);
            }
            catch (OperationCanceledException)
            {
                throw;
            }
            catch (Exception ex)
            {
                return new CoreConnectionStatus(false, $"Core socket {SocketPath} refused the connection: {ex.Message}");
            }
        }
    }
}
