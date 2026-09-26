using Lively.Common.Services;
using NLog;
using System;
using System.Buffers.Binary;
using System.Diagnostics;
using System.Linq;
using System.Text;
using System.Threading;

namespace Lively.Common.Linux.Audio
{
    /// <summary>
    /// Captures the monitor of a PulseAudio / PipeWire-Pulse sink through <c>parec</c> and emits
    /// <see cref="SpectrumProcessor.BinCount"/> smoothed FFT magnitudes per 1024-frame chunk.
    /// When started without a sink name the default sink is used and followed across default-sink changes.
    /// </summary>
    public sealed class PulseAudioVisualizerService : IAudioVisualizerService
    {
        private static readonly Logger logger = LogManager.GetCurrentClassLogger();
        private const int SampleRate = 44100;
        private const int LatencyMs = 30;

        public event EventHandler<double[]> AudioDataAvailable;

        private readonly object sync = new();
        private Capture capture;
        private DefaultSinkWatcher defaultSinkWatcher;
        private bool disposed;

        /// <summary>
        /// Starts capturing <paramref name="deviceId"/>'s monitor source, or the default sink's monitor when null or empty.
        /// </summary>
        /// <exception cref="ArgumentException">The named sink does not exist.</exception>
        /// <exception cref="InvalidOperationException">pactl could not report the default sink.</exception>
        public void Start(string deviceId = null)
        {
            Stop();

            var sinkName = string.IsNullOrWhiteSpace(deviceId) ? null : deviceId.Trim();
            if (sinkName != null)
                EnsureSinkExists(sinkName);
            var followDefault = sinkName == null;
            sinkName ??= PulseAudioDevices.GetDefaultSink();

            lock (sync)
            {
                ThrowIfDisposed();
                capture = Capture.Start(sinkName, SampleRate, LatencyMs, RaiseAudioData, OnCaptureExited);
                if (followDefault)
                    defaultSinkWatcher = DefaultSinkWatcher.Start(sinkName, OnDefaultSinkChanged);
            }
            logger.Info("Audio visualizer capturing {0}.monitor via parec (follow default sink: {1}).", sinkName, followDefault);
        }

        public void Stop()
        {
            Capture oldCapture;
            DefaultSinkWatcher oldWatcher;
            lock (sync)
            {
                oldCapture = capture;
                capture = null;
                oldWatcher = defaultSinkWatcher;
                defaultSinkWatcher = null;
            }
            oldWatcher?.Dispose();
            oldCapture?.Dispose();
        }

        public void Dispose()
        {
            lock (sync)
            {
                if (disposed)
                    return;
                disposed = true;
            }
            Stop();
        }

        private static void EnsureSinkExists(string sinkName)
        {
            var sinks = PulseAudioDevices.ListSinks();
            if (sinks.Any(s => string.Equals(s.name, sinkName, StringComparison.Ordinal)))
                return;
            throw new ArgumentException(
                $"Audio sink '{sinkName}' was not found. Available sinks: {string.Join(", ", sinks.Select(s => s.name))}",
                nameof(sinkName));
        }

        private void OnDefaultSinkChanged(DefaultSinkWatcher source, string newSinkName)
        {
            Capture oldCapture;
            lock (sync)
            {
                if (disposed || !ReferenceEquals(defaultSinkWatcher, source))
                    return;
                oldCapture = capture;
                capture = null;
                oldCapture?.RequestStop();
                capture = Capture.Start(newSinkName, SampleRate, LatencyMs, RaiseAudioData, OnCaptureExited);
            }
            oldCapture?.Dispose();
            logger.Info("Default sink changed; audio visualizer now capturing {0}.monitor.", newSinkName);
        }

        private void OnCaptureExited(Capture source, int exitCode, string stderr)
        {
            lock (sync)
            {
                if (!ReferenceEquals(capture, source))
                    return;
                capture = null;
            }
            logger.Error("parec for {0}.monitor exited unexpectedly with code {1}: {2}", source.SinkName, exitCode, stderr);
            RaiseAudioData(SpectrumProcessor.Silence());
            source.Dispose();
        }

        private void RaiseAudioData(double[] data)
        {
            try
            {
                AudioDataAvailable?.Invoke(this, data);
            }
            catch (Exception ex)
            {
                logger.Error(ex, "An AudioDataAvailable handler threw.");
            }
        }

        private void ThrowIfDisposed()
        {
            if (disposed)
                throw new ObjectDisposedException(nameof(PulseAudioVisualizerService));
        }

        /// <summary>One running parec process plus the thread that turns its raw float stream into spectra.</summary>
        private sealed class Capture : IDisposable
        {
            private readonly Process process;
            private readonly Thread reader;
            private readonly Action<double[]> onFrame;
            private readonly Action<Capture, int, string> onUnexpectedExit;
            private readonly SpectrumProcessor processor = new();
            private readonly StringBuilder stderr = new();
            private volatile bool stopRequested;

            public string SinkName { get; }

            private Capture(string sinkName, Process process, Action<double[]> onFrame, Action<Capture, int, string> onUnexpectedExit)
            {
                SinkName = sinkName;
                this.process = process;
                this.onFrame = onFrame;
                this.onUnexpectedExit = onUnexpectedExit;
                reader = new Thread(ReadLoop) { IsBackground = true, Name = "parec spectrum reader" };
            }

            public static Capture Start(string sinkName, int sampleRate, int latencyMs, Action<double[]> onFrame, Action<Capture, int, string> onUnexpectedExit)
            {
                var psi = new ProcessStartInfo("parec")
                {
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    RedirectStandardInput = false,
                    CreateNoWindow = true,
                };
                psi.ArgumentList.Add("--format=float32le");
                psi.ArgumentList.Add($"--rate={sampleRate}");
                psi.ArgumentList.Add("--channels=1");
                psi.ArgumentList.Add($"--latency-msec={latencyMs}");
                psi.ArgumentList.Add("--raw");
                psi.ArgumentList.Add("-d");
                psi.ArgumentList.Add(sinkName + ".monitor");

                var process = Process.Start(psi)
                    ?? throw new InvalidOperationException("Failed to start parec.");
                var capture = new Capture(sinkName, process, onFrame, onUnexpectedExit);
                process.ErrorDataReceived += capture.OnStderrLine;
                process.BeginErrorReadLine();
                capture.reader.Start();
                return capture;
            }

            public void RequestStop()
            {
                stopRequested = true;
                KillProcess();
            }

            public void Dispose()
            {
                RequestStop();
                if (reader.IsAlive && Thread.CurrentThread != reader)
                    reader.Join();
                process.Dispose();
            }

            private void OnStderrLine(object sender, DataReceivedEventArgs e)
            {
                if (e.Data is null)
                    return;
                lock (stderr)
                    stderr.AppendLine(e.Data);
            }

            private void KillProcess()
            {
                try
                {
                    if (!process.HasExited)
                        process.Kill(entireProcessTree: true);
                }
                catch (InvalidOperationException ex)
                {
                    logger.Debug(ex, "parec already exited while stopping.");
                }
            }

            private void ReadLoop()
            {
                var stream = process.StandardOutput.BaseStream;
                var bytes = new byte[SpectrumProcessor.RequiredSampleCount * sizeof(float)];
                var samples = new float[SpectrumProcessor.RequiredSampleCount];
                try
                {
                    while (!stopRequested)
                    {
                        int filled = 0;
                        while (filled < bytes.Length)
                        {
                            int read = stream.Read(bytes, filled, bytes.Length - filled);
                            if (read <= 0)
                                break;
                            filled += read;
                        }
                        if (filled < bytes.Length)
                            break;

                        for (int i = 0; i < samples.Length; i++)
                            samples[i] = BinaryPrimitives.ReadSingleLittleEndian(bytes.AsSpan(i * sizeof(float), sizeof(float)));
                        onFrame(processor.Process(samples));
                    }
                }
                catch (Exception ex)
                {
                    if (stopRequested)
                        logger.Debug(ex, "parec stream closed while stopping.");
                    else
                        logger.Error(ex, "Reading the parec stream failed.");
                }

                if (stopRequested)
                    return;

                process.WaitForExit();
                string stderrText;
                lock (stderr)
                    stderrText = stderr.ToString().Trim();
                onUnexpectedExit(this, process.ExitCode, stderrText);
            }
        }

        /// <summary>Runs <c>pactl subscribe</c> and reports when the default sink name changes.</summary>
        private sealed class DefaultSinkWatcher : IDisposable
        {
            private readonly Process process;
            private readonly Thread reader;
            private readonly Action<DefaultSinkWatcher, string> onDefaultSinkChanged;
            private string currentSinkName;
            private volatile bool stopRequested;

            private DefaultSinkWatcher(Process process, string currentSinkName, Action<DefaultSinkWatcher, string> onDefaultSinkChanged)
            {
                this.process = process;
                this.currentSinkName = currentSinkName;
                this.onDefaultSinkChanged = onDefaultSinkChanged;
                reader = new Thread(ReadLoop) { IsBackground = true, Name = "pactl subscribe reader" };
            }

            public static DefaultSinkWatcher Start(string currentSinkName, Action<DefaultSinkWatcher, string> onDefaultSinkChanged)
            {
                var psi = new ProcessStartInfo("pactl")
                {
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    CreateNoWindow = true,
                };
                psi.ArgumentList.Add("subscribe");
                var process = Process.Start(psi)
                    ?? throw new InvalidOperationException("Failed to start pactl subscribe.");
                process.ErrorDataReceived += (s, e) =>
                {
                    if (e.Data != null)
                        logger.Warn("pactl subscribe: {0}", e.Data);
                };
                process.BeginErrorReadLine();
                var watcher = new DefaultSinkWatcher(process, currentSinkName, onDefaultSinkChanged);
                watcher.reader.Start();
                return watcher;
            }

            public void Dispose()
            {
                stopRequested = true;
                try
                {
                    if (!process.HasExited)
                        process.Kill(entireProcessTree: true);
                }
                catch (InvalidOperationException ex)
                {
                    logger.Debug(ex, "pactl subscribe already exited while stopping.");
                }
                if (reader.IsAlive && Thread.CurrentThread != reader)
                    reader.Join();
                process.Dispose();
            }

            private void ReadLoop()
            {
                try
                {
                    string line;
                    while (!stopRequested && (line = process.StandardOutput.ReadLine()) != null)
                    {
                        if (!line.Contains("'change' on server", StringComparison.Ordinal))
                            continue;
                        var sinkName = PulseAudioDevices.GetDefaultSink();
                        if (string.Equals(sinkName, currentSinkName, StringComparison.Ordinal))
                            continue;
                        currentSinkName = sinkName;
                        onDefaultSinkChanged(this, sinkName);
                    }
                }
                catch (Exception ex)
                {
                    if (stopRequested)
                        logger.Debug(ex, "pactl subscribe stream closed while stopping.");
                    else
                        logger.Error(ex, "Following the default sink failed.");
                    return;
                }
                if (!stopRequested)
                {
                    process.WaitForExit();
                    logger.Error("pactl subscribe exited unexpectedly with code {0}; the audio visualizer stays on {1}.monitor.", process.ExitCode, currentSinkName);
                }
            }
        }
    }
}
