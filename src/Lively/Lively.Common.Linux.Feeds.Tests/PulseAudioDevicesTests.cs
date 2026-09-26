using Lively.Common.Linux.Audio;
using System;
using System.Linq;
using Xunit;

namespace Lively.Common.Linux.Feeds.Tests
{
    public class PulseAudioDevicesTests
    {
        // Captured from `pactl -f json list sinks` on a PipeWire 1.x system (properties trimmed).
        private const string CapturedSinks = """
        [
          {
            "index": 61,
            "state": "SUSPENDED",
            "name": "alsa_output.pci-0000_01_00.1.hdmi-stereo",
            "description": "GA102 High Definition Audio Controller Digital Stereo (HDMI)",
            "driver": "PipeWire",
            "sample_specification": "s16le 2ch 48000Hz",
            "channel_map": "front-left,front-right",
            "owner_module": 4294967295,
            "mute": false,
            "volume": {
              "front-left": { "value": 39977, "value_percent": "61%", "db": "-12.88 dB" },
              "front-right": { "value": 39977, "value_percent": "61%", "db": "-12.88 dB" }
            },
            "balance": 0.0,
            "base_volume": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" },
            "monitor_source": "alsa_output.pci-0000_01_00.1.hdmi-stereo.monitor",
            "latency": { "actual": 0.0, "configured": 0.0 },
            "flags": [ "HARDWARE", "DECIBEL_VOLUME", "LATENCY", "SET_FORMATS" ],
            "properties": {
              "object.path": "alsa:acp:NVidia:4:playback",
              "api.alsa.path": "hdmi:1",
              "api.alsa.pcm.card": "1",
              "api.alsa.pcm.stream": "playback"
            },
            "ports": [
              {
                "name": "hdmi-output-0",
                "description": "HDMI / DisplayPort",
                "type": "HDMI",
                "priority": 5900,
                "availability_group": "Legacy 1",
                "availability": "available"
              }
            ],
            "active_port": "hdmi-output-0"
          },
          {
            "index": 67,
            "state": "RUNNING",
            "name": "alsa_output.usb-Audeze_LLC_Audeze_Maxwell_XBOX_Dongle_0000000000000000-01.analog-stereo",
            "description": "Audeze Maxwell XBOX Dongle Analog Stereo",
            "driver": "PipeWire",
            "sample_specification": "s24le 2ch 48000Hz",
            "channel_map": "front-left,front-right",
            "owner_module": 4294967295,
            "mute": false,
            "volume": {
              "front-left": { "value": 55706, "value_percent": "85%", "db": "-4.23 dB" },
              "front-right": { "value": 55706, "value_percent": "85%", "db": "-4.23 dB" }
            },
            "balance": 0.0,
            "base_volume": { "value": 68100, "value_percent": "104%", "db": "1.00 dB" },
            "monitor_source": "alsa_output.usb-Audeze_LLC_Audeze_Maxwell_XBOX_Dongle_0000000000000000-01.analog-stereo.monitor",
            "latency": { "actual": 0.0, "configured": 0.0 },
            "flags": [ "HARDWARE", "HW_MUTE_CTRL", "HW_VOLUME_CTRL", "DECIBEL_VOLUME", "LATENCY" ],
            "properties": {
              "object.path": "alsa:acp:Dongle:4:playback",
              "api.alsa.path": "front:3",
              "api.alsa.pcm.card": "3",
              "api.alsa.pcm.stream": "playback"
            },
            "ports": [
              {
                "name": "analog-output",
                "description": "Analog Output",
                "type": "Analog",
                "priority": 9900,
                "availability_group": "",
                "availability": "availability unknown"
              }
            ],
            "active_port": "analog-output"
          }
        ]
        """;

        [Fact]
        public void ParsesNamesDescriptionsAndDefaultFlag()
        {
            var sinks = PulseAudioDevices.ParseSinks(CapturedSinks, "alsa_output.usb-Audeze_LLC_Audeze_Maxwell_XBOX_Dongle_0000000000000000-01.analog-stereo");

            Assert.Equal(2, sinks.Count);
            Assert.Equal("alsa_output.pci-0000_01_00.1.hdmi-stereo", sinks[0].name);
            Assert.Equal("GA102 High Definition Audio Controller Digital Stereo (HDMI)", sinks[0].description);
            Assert.False(sinks[0].isDefault);
            Assert.Equal("alsa_output.usb-Audeze_LLC_Audeze_Maxwell_XBOX_Dongle_0000000000000000-01.analog-stereo", sinks[1].name);
            Assert.Equal("Audeze Maxwell XBOX Dongle Analog Stereo", sinks[1].description);
            Assert.True(sinks[1].isDefault);
        }

        [Fact]
        public void NoSinkIsDefaultWhenTheDefaultNameIsUnknown()
        {
            var sinks = PulseAudioDevices.ParseSinks(CapturedSinks, "some.other.sink");
            Assert.All(sinks, s => Assert.False(s.isDefault));
        }

        [Fact]
        public void EmptyArrayGivesNoSinks()
        {
            Assert.Empty(PulseAudioDevices.ParseSinks("[]", "x"));
        }

        [Fact]
        public void SinkWithoutNameIsRejected()
        {
            Assert.Throws<FormatException>(() => PulseAudioDevices.ParseSinks("""[{"index": 1, "description": "nameless"}]""", "x"));
        }

        [Fact]
        public void DescriptionDefaultsToNameWhenMissing()
        {
            var sinks = PulseAudioDevices.ParseSinks("""[{"index": 1, "name": "only.name"}]""", "only.name");
            Assert.Equal("only.name", sinks.Single().description);
            Assert.True(sinks.Single().isDefault);
        }
    }
}
