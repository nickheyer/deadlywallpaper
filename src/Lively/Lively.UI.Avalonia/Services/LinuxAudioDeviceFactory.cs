using Lively.Common.Linux.Audio;
using Lively.Models;
using Lively.UI.Shared.Factories;
using System;
using System.Collections.Generic;
using System.Linq;

namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// Enumerates PulseAudio/PipeWire output devices through <see cref="PulseAudioDevices"/>.
    /// </summary>
    public class LinuxAudioDeviceFactory : IAudioDeviceFactory
    {
        public AudioDevice GetDefaultRenderDevice()
        {
            var sink = PulseAudioDevices.ListSinks().FirstOrDefault(x => x.isDefault);
            if (sink.name is null)
                throw new InvalidOperationException("The sound server reports no default sink.");

            return CreateDevice(sink.name, sink.description);
        }

        public IEnumerable<AudioDevice> GetRenderDevices()
        {
            return PulseAudioDevices.ListSinks().Select(x => CreateDevice(x.name, x.description)).ToList();
        }

        public AudioDevice GetDeviceById(string id)
        {
            try
            {
                return GetRenderDevices().FirstOrDefault(x => x.Id == id);
            }
            catch (Exception)
            {
                // The sound server is not reachable, the device cannot be resolved.
                return null;
            }
        }

        private static AudioDevice CreateDevice(string name, string description)
        {
            return new AudioDevice(name, description, LinuxIconLookup.ResolveIcon("audio-card"));
        }
    }
}
