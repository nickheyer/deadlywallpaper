using Newtonsoft.Json.Linq;
using System;
using System.Collections.Generic;
using System.Diagnostics;

namespace Lively.Common.Linux.Audio
{
    /// <summary>
    /// Enumerates PulseAudio / PipeWire-Pulse sinks through <c>pactl</c>.
    /// </summary>
    public static class PulseAudioDevices
    {
        private const int PactlTimeoutMs = 5000;

        /// <summary>
        /// Lists every sink known to the sound server; <c>isDefault</c> marks the one <c>pactl get-default-sink</c> reports.
        /// </summary>
        public static IReadOnlyList<(string name, string description, bool isDefault)> ListSinks()
        {
            var json = RunPactl("-f", "json", "list", "sinks");
            var defaultSink = GetDefaultSink();
            return ParseSinks(json, defaultSink);
        }

        /// <summary>The name of the sink <c>pactl get-default-sink</c> reports.</summary>
        public static string GetDefaultSink()
        {
            var name = RunPactl("get-default-sink").Trim();
            if (name.Length == 0)
                throw new InvalidOperationException("pactl get-default-sink returned no sink name.");
            return name;
        }

        /// <summary>Parses the output of <c>pactl -f json list sinks</c>.</summary>
        public static IReadOnlyList<(string name, string description, bool isDefault)> ParseSinks(string pactlJson, string defaultSinkName)
        {
            if (pactlJson is null)
                throw new ArgumentNullException(nameof(pactlJson));

            var sinks = JArray.Parse(pactlJson);
            var result = new List<(string name, string description, bool isDefault)>(sinks.Count);
            foreach (var sink in sinks)
            {
                var name = sink.Value<string>("name");
                if (string.IsNullOrEmpty(name))
                    throw new FormatException($"pactl sink entry without a name: {sink.ToString(Newtonsoft.Json.Formatting.None)}");
                var description = sink.Value<string>("description") ?? name;
                result.Add((name, description, string.Equals(name, defaultSinkName, StringComparison.Ordinal)));
            }
            return result;
        }

        internal static string RunPactl(params string[] args)
        {
            var psi = new ProcessStartInfo("pactl")
            {
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                CreateNoWindow = true,
            };
            foreach (var arg in args)
                psi.ArgumentList.Add(arg);

            using var process = Process.Start(psi)
                ?? throw new InvalidOperationException("Failed to start pactl.");
            var stdoutTask = process.StandardOutput.ReadToEndAsync();
            var stderrTask = process.StandardError.ReadToEndAsync();
            if (!process.WaitForExit(PactlTimeoutMs))
            {
                process.Kill(entireProcessTree: true);
                throw new TimeoutException($"pactl {string.Join(' ', args)} did not finish within {PactlTimeoutMs} ms.");
            }
            process.WaitForExit();
            var stdout = stdoutTask.GetAwaiter().GetResult();
            var stderr = stderrTask.GetAwaiter().GetResult();
            if (process.ExitCode != 0)
                throw new InvalidOperationException($"pactl {string.Join(' ', args)} failed with exit code {process.ExitCode}: {stderr.Trim()}");
            return stdout;
        }
    }
}
