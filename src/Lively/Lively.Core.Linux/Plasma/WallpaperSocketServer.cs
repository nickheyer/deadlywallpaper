using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Plasma
{
    /// <summary>
    /// A connection from one Plasma wallpaper instance (PROTOCOL.md section 7).
    /// </summary>
    public sealed class WallpaperConnection
    {
        private readonly Socket socket;
        private readonly SemaphoreSlim sendLock = new SemaphoreSlim(1, 1);

        public string Instance { get; }
        public bool IsOpen { get; private set; } = true;

        public event EventHandler<string> MessageReceived;
        public event EventHandler Closed;

        internal WallpaperConnection(string instance, Socket socket)
        {
            Instance = instance;
            this.socket = socket;
        }

        public async Task SendAsync(string text)
        {
            if (!IsOpen)
                return;
            var payload = Encoding.UTF8.GetBytes(text);
            var frame = WebSocketFraming.EncodeServerFrame(0x1, payload);
            await sendLock.WaitAsync();
            try
            {
                await socket.SendAsync(frame, SocketFlags.None);
            }
            catch (SocketException)
            {
                MarkClosed();
            }
            catch (ObjectDisposedException)
            {
                MarkClosed();
            }
            finally
            {
                sendLock.Release();
            }
        }

        internal void OnMessage(string text) => MessageReceived?.Invoke(this, text);

        internal async Task SendPongAsync(byte[] payload)
        {
            await sendLock.WaitAsync();
            try { await socket.SendAsync(WebSocketFraming.EncodeServerFrame(0xA, payload), SocketFlags.None); }
            catch (SocketException) { MarkClosed(); }
            catch (ObjectDisposedException) { MarkClosed(); }
            finally { sendLock.Release(); }
        }

        internal void MarkClosed()
        {
            if (!IsOpen)
                return;
            IsOpen = false;
            try { socket.Shutdown(SocketShutdown.Both); } catch (SocketException) { } catch (ObjectDisposedException) { }
            socket.Dispose();
            Closed?.Invoke(this, EventArgs.Empty);
        }

        public void Close() => MarkClosed();
    }

    /// <summary>
    /// Minimal RFC 6455 server on 127.0.0.1 used by the Plasma wallpaper plugin to talk to the core.
    /// Only text frames are used by the protocol; ping/pong and close are handled.
    /// </summary>
    public sealed class WallpaperSocketServer : IDisposable
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private const string WebSocketGuid = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

        private readonly ConcurrentDictionary<string, Action<WallpaperConnection>> instances = new ConcurrentDictionary<string, Action<WallpaperConnection>>();
        private readonly CancellationTokenSource cts = new CancellationTokenSource();
        private TcpListener listener;

        public int Port { get; private set; }
        public string Url => $"ws://127.0.0.1:{Port}";

        public void Start()
        {
            listener = new TcpListener(IPAddress.Loopback, 0);
            listener.Start();
            Port = ((IPEndPoint)listener.LocalEndpoint).Port;
            Logger.Info($"Wallpaper socket server listening on {Url}");
            _ = Task.Run(AcceptLoopAsync);
        }

        /// <summary>
        /// Registers the handler that receives the connection for <paramref name="instance"/>.
        /// </summary>
        public void Register(string instance, Action<WallpaperConnection> onConnected) => instances[instance] = onConnected;

        public void Unregister(string instance) => instances.TryRemove(instance, out _);

        private async Task AcceptLoopAsync()
        {
            while (!cts.IsCancellationRequested)
            {
                Socket socket;
                try
                {
                    socket = await listener.AcceptSocketAsync(cts.Token);
                }
                catch (OperationCanceledException)
                {
                    return;
                }
                catch (ObjectDisposedException)
                {
                    return;
                }
                _ = Task.Run(() => HandleClientAsync(socket));
            }
        }

        private async Task HandleClientAsync(Socket socket)
        {
            WallpaperConnection connection = null;
            try
            {
                socket.NoDelay = true;
                var stream = new NetworkStream(socket, ownsSocket: false);
                var request = await ReadHttpRequestAsync(stream);
                if (request == null)
                {
                    socket.Dispose();
                    return;
                }

                var (path, key) = request.Value;
                const string prefix = "/wallpaper/";
                if (!path.StartsWith(prefix, StringComparison.Ordinal) || key == null)
                {
                    await WriteHttpAsync(stream, "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
                    socket.Dispose();
                    return;
                }

                var instance = path.Substring(prefix.Length);
                if (!instances.TryGetValue(instance, out var onConnected))
                {
                    Logger.Warn($"Wallpaper socket: connection for unknown instance '{instance}' refused.");
                    await WriteHttpAsync(stream, "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
                    socket.Dispose();
                    return;
                }

                var accept = Convert.ToBase64String(SHA1.HashData(Encoding.ASCII.GetBytes(key + WebSocketGuid)));
                await WriteHttpAsync(stream, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: " + accept + "\r\n\r\n");

                connection = new WallpaperConnection(instance, socket);
                onConnected(connection);

                var reader = new WebSocketFraming.FrameReader(stream);
                var fragments = new List<byte>();
                while (connection.IsOpen)
                {
                    var frame = await reader.ReadFrameAsync(cts.Token);
                    if (frame == null)
                        break;

                    switch (frame.Opcode)
                    {
                        case 0x1: // text
                        case 0x0: // continuation
                            fragments.AddRange(frame.Payload);
                            if (frame.Fin)
                            {
                                var text = Encoding.UTF8.GetString(fragments.ToArray());
                                fragments.Clear();
                                connection.OnMessage(text);
                            }
                            break;
                        case 0x8: // close
                            connection.MarkClosed();
                            break;
                        case 0x9: // ping
                            await connection.SendPongAsync(frame.Payload);
                            break;
                        case 0xA: // pong
                            break;
                        default:
                            Logger.Warn($"Wallpaper socket: unsupported opcode {frame.Opcode}; closing.");
                            connection.MarkClosed();
                            break;
                    }
                }
            }
            catch (IOException ex)
            {
                Logger.Info($"Wallpaper socket connection ended: {ex.Message}");
            }
            catch (SocketException ex)
            {
                Logger.Info($"Wallpaper socket connection ended: {ex.Message}");
            }
            catch (OperationCanceledException)
            {
            }
            finally
            {
                if (connection != null)
                    connection.MarkClosed();
                else
                    socket.Dispose();
            }
        }

        private static async Task<(string path, string key)?> ReadHttpRequestAsync(NetworkStream stream)
        {
            var buffer = new byte[8192];
            var total = 0;
            while (total < buffer.Length)
            {
                var n = await stream.ReadAsync(buffer, total, buffer.Length - total);
                if (n == 0)
                    return null;
                total += n;
                var text = Encoding.ASCII.GetString(buffer, 0, total);
                var end = text.IndexOf("\r\n\r\n", StringComparison.Ordinal);
                if (end < 0)
                    continue;

                var lines = text.Substring(0, end).Split("\r\n");
                var requestLine = lines[0].Split(' ');
                if (requestLine.Length < 2 || requestLine[0] != "GET")
                    return (string.Empty, null);
                string key = null;
                foreach (var line in lines)
                {
                    var colon = line.IndexOf(':');
                    if (colon > 0 && line.Substring(0, colon).Trim().Equals("Sec-WebSocket-Key", StringComparison.OrdinalIgnoreCase))
                        key = line.Substring(colon + 1).Trim();
                }
                return (requestLine[1], key);
            }
            return null;
        }

        private static Task WriteHttpAsync(NetworkStream stream, string response)
        {
            var bytes = Encoding.ASCII.GetBytes(response);
            return stream.WriteAsync(bytes, 0, bytes.Length);
        }

        public void Dispose()
        {
            cts.Cancel();
            listener?.Stop();
        }
    }

    /// <summary>RFC 6455 frame encoding and decoding (server side: incoming frames are masked, outgoing are not).</summary>
    public static class WebSocketFraming
    {
        public sealed class Frame
        {
            public bool Fin { get; set; }
            public int Opcode { get; set; }
            public byte[] Payload { get; set; }
        }

        public static byte[] EncodeServerFrame(int opcode, byte[] payload)
        {
            var header = new List<byte> { (byte)(0x80 | (opcode & 0x0F)) };
            if (payload.Length < 126)
            {
                header.Add((byte)payload.Length);
            }
            else if (payload.Length <= ushort.MaxValue)
            {
                header.Add(126);
                header.Add((byte)(payload.Length >> 8));
                header.Add((byte)(payload.Length & 0xFF));
            }
            else
            {
                header.Add(127);
                for (var i = 7; i >= 0; i--)
                    header.Add((byte)((long)payload.Length >> (8 * i) & 0xFF));
            }
            var frame = new byte[header.Count + payload.Length];
            header.CopyTo(frame, 0);
            Buffer.BlockCopy(payload, 0, frame, header.Count, payload.Length);
            return frame;
        }

        public static Frame DecodeFrame(byte[] data, out int consumed)
        {
            consumed = 0;
            if (data.Length < 2)
                return null;
            var fin = (data[0] & 0x80) != 0;
            var opcode = data[0] & 0x0F;
            var masked = (data[1] & 0x80) != 0;
            long length = data[1] & 0x7F;
            var offset = 2;
            if (length == 126)
            {
                if (data.Length < 4) return null;
                length = (data[2] << 8) | data[3];
                offset = 4;
            }
            else if (length == 127)
            {
                if (data.Length < 10) return null;
                length = 0;
                for (var i = 0; i < 8; i++)
                    length = (length << 8) | data[2 + i];
                offset = 10;
            }
            byte[] mask = null;
            if (masked)
            {
                if (data.Length < offset + 4) return null;
                mask = new byte[4];
                Buffer.BlockCopy(data, offset, mask, 0, 4);
                offset += 4;
            }
            if (data.Length < offset + length)
                return null;
            var payload = new byte[length];
            Buffer.BlockCopy(data, offset, payload, 0, (int)length);
            if (mask != null)
                for (var i = 0; i < payload.Length; i++)
                    payload[i] ^= mask[i % 4];
            consumed = offset + (int)length;
            return new Frame { Fin = fin, Opcode = opcode, Payload = payload };
        }

        public sealed class FrameReader
        {
            private readonly Stream stream;
            private readonly List<byte> pending = new List<byte>();
            private readonly byte[] buffer = new byte[16384];

            public FrameReader(Stream stream) => this.stream = stream;

            public async Task<Frame> ReadFrameAsync(CancellationToken token)
            {
                while (true)
                {
                    var frame = DecodeFrame(pending.ToArray(), out var consumed);
                    if (frame != null)
                    {
                        pending.RemoveRange(0, consumed);
                        return frame;
                    }
                    var n = await stream.ReadAsync(buffer, 0, buffer.Length, token);
                    if (n == 0)
                        return null;
                    for (var i = 0; i < n; i++)
                        pending.Add(buffer[i]);
                }
            }
        }
    }
}
