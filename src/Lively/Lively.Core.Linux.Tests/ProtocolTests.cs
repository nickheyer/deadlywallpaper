using Lively.Core.Linux.Hosting;
using Lively.Core.Linux.Plasma;
using Lively.Core.Linux.Wallpapers;
using Lively.Models.Message;
using System.Drawing;
using System.Text;
using Xunit;

namespace Lively.Core.Linux.Tests
{
    public class HostProtocolTests
    {
        [Fact]
        public void Serializes_type_first_and_parses_back()
        {
            var line = HostProtocol.Serialize(new LivelyVolumeCmd { Volume = 42 });
            Assert.StartsWith("{\"Type\":9", line);

            var parsed = HostProtocol.TryParse("{\"Type\":3,\"FileName\":\"a.jpg\",\"Success\":true}");
            var shot = Assert.IsType<LivelyMessageScreenshot>(parsed);
            Assert.Equal("a.jpg", shot.FileName);
            Assert.True(shot.Success);
        }

        [Fact]
        public void Mpv_command_uses_type_100()
        {
            var line = HostProtocol.Serialize(new HostMpvCommand("seek", 50f, "absolute-percent"));
            Assert.Contains("\"Type\":100", line);
            Assert.Contains("\"Command\":[\"seek\",50.0,\"absolute-percent\"]", line);
        }

        [Fact]
        public void Audio_message_round_trips()
        {
            var line = HostProtocol.Serialize(new LivelySystemAudio { Data = new[] { 0.1, 0.2 } });
            var parsed = Assert.IsType<LivelySystemAudio>(HostProtocol.TryParse(line));
            Assert.Equal(2, parsed.Data.Length);
        }

        [Theory]
        [InlineData("")]
        [InlineData("plain log text")]
        [InlineData("{\"NoType\":1}")]
        [InlineData("{not json")]
        public void Non_messages_return_null(string line)
        {
            Assert.Null(HostProtocol.TryParse(line));
        }
    }

    public class SpanGeometryTests
    {
        [Fact]
        public void Argument_is_relative_to_virtual_origin()
        {
            var span = new SpanGeometry(new Rectangle(2649, 0, 1080, 1920), new Rectangle(0, 0, 3729, 2056));
            Assert.Equal("2649,0,1080,1920,3729,2056", span.ToArgument());

            var negative = new SpanGeometry(new Rectangle(-1920, 0, 1920, 1080), new Rectangle(-1920, 0, 3840, 1080));
            Assert.Equal("0,0,1920,1080,3840,1080", negative.ToArgument());
        }
    }

    public class WallpaperArgumentsTests
    {
        [Fact]
        public void Accepts_prefixed_and_unprefixed_flags()
        {
            var args = WallpaperArguments.Parse("--system-information true --wallpaper-pause-event --nowplaying false --system-nowplaying true --pause-media");
            Assert.True(args.SystemInformation);
            Assert.True(args.PauseEvent);
            Assert.True(args.NowPlaying);
            Assert.True(args.PauseMedia);
            Assert.False(args.VerboseLog);
        }

        [Fact]
        public void Empty_arguments_mean_nothing_requested()
        {
            var args = WallpaperArguments.Parse(null);
            Assert.False(args.SystemInformation);
            Assert.False(args.NowPlaying);
        }
    }

    public class WebSocketFramingTests
    {
        [Fact]
        public void Server_frames_are_unmasked_and_decode()
        {
            var payload = Encoding.UTF8.GetBytes("{\"Type\":7}");
            var frame = WebSocketFraming.EncodeServerFrame(0x1, payload);
            Assert.Equal(0x81, frame[0]);
            Assert.Equal(payload.Length, frame[1]);

            var decoded = WebSocketFraming.DecodeFrame(frame, out var consumed);
            Assert.NotNull(decoded);
            Assert.Equal(frame.Length, consumed);
            Assert.True(decoded.Fin);
            Assert.Equal(1, decoded.Opcode);
            Assert.Equal(payload, decoded.Payload);
        }

        [Fact]
        public void Masked_client_frame_is_unmasked()
        {
            var payload = Encoding.UTF8.GetBytes("hello");
            var mask = new byte[] { 1, 2, 3, 4 };
            var frame = new byte[2 + 4 + payload.Length];
            frame[0] = 0x81;
            frame[1] = (byte)(0x80 | payload.Length);
            mask.CopyTo(frame, 2);
            for (var i = 0; i < payload.Length; i++)
                frame[6 + i] = (byte)(payload[i] ^ mask[i % 4]);

            var decoded = WebSocketFraming.DecodeFrame(frame, out _);
            Assert.Equal("hello", Encoding.UTF8.GetString(decoded.Payload));
        }

        [Fact]
        public void Long_payloads_use_extended_length()
        {
            var payload = new byte[70000];
            var frame = WebSocketFraming.EncodeServerFrame(0x1, payload);
            Assert.Equal(127, frame[1]);
            var decoded = WebSocketFraming.DecodeFrame(frame, out var consumed);
            Assert.Equal(frame.Length, consumed);
            Assert.Equal(70000, decoded.Payload.Length);
        }

        [Fact]
        public void Incomplete_frame_returns_null()
        {
            var frame = WebSocketFraming.EncodeServerFrame(0x1, new byte[10]);
            var partial = new byte[5];
            System.Array.Copy(frame, partial, 5);
            Assert.Null(WebSocketFraming.DecodeFrame(partial, out _));
        }
    }

    public class PlasmaScriptingTests
    {
        [Fact]
        public void Js_literals_are_escaped()
        {
            Assert.Equal("\"a \\\"quoted\\\" path\"", PlasmaShellScripting.JsLiteral("a \"quoted\" path"));
            Assert.Equal("true", PlasmaShellScripting.JsLiteral(true));
            Assert.Equal("42", PlasmaShellScripting.JsLiteral(42));
            Assert.Equal("1.5", PlasmaShellScripting.JsLiteral(1.5));
            Assert.Equal("''", PlasmaShellScripting.JsLiteral(null));
        }

        [Fact]
        public void Kind_names_match_plugin_config()
        {
            Assert.Equal("video", PlasmaWallpaper.KindName(Models.Enums.WallpaperType.video));
            Assert.Equal("web", PlasmaWallpaper.KindName(Models.Enums.WallpaperType.webaudio));
            Assert.Equal("none", PlasmaWallpaper.KindName(Models.Enums.WallpaperType.app));
        }
    }
}
