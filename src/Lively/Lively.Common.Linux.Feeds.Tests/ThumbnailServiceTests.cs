using ImageMagick;
using Lively.Common.Linux.Media;
using System;
using System.Diagnostics;
using System.IO;
using System.Threading.Tasks;
using Xunit;

namespace Lively.Common.Linux.Feeds.Tests
{
    public class ThumbnailServiceTests : IDisposable
    {
        private readonly string workDir = Path.Combine(Path.GetTempPath(), "lively-feeds-thumbs-" + Guid.NewGuid().ToString("N"));
        private readonly LinuxThumbnailService service = new();

        public ThumbnailServiceTests()
        {
            Directory.CreateDirectory(workDir);
        }

        public void Dispose()
        {
            Directory.Delete(workDir, recursive: true);
        }

        private string GeneratePng(string fileName, uint width, uint height, bool transparent)
        {
            var path = Path.Combine(workDir, fileName);
            using var image = new MagickImage(transparent ? MagickColors.Transparent : MagickColors.OrangeRed, width, height);
            image.Format = MagickFormat.Png;
            image.Write(path);
            return path;
        }

        private string GenerateVideo(string fileName, double durationSeconds, string extraInputArgs = "")
        {
            var path = Path.Combine(workDir, fileName);
            var psi = new ProcessStartInfo("ffmpeg", $"-y -hide_banner -loglevel error -f lavfi -i testsrc=duration={durationSeconds.ToString(System.Globalization.CultureInfo.InvariantCulture)}:size=320x240:rate=10 {extraInputArgs} \"{path}\"")
            {
                UseShellExecute = false,
                RedirectStandardError = true,
            };
            using var process = Process.Start(psi);
            var stderr = process.StandardError.ReadToEnd();
            process.WaitForExit();
            Assert.True(process.ExitCode == 0, $"ffmpeg could not generate the test clip: {stderr}");
            return path;
        }

        private static (uint width, uint height, MagickFormat format) Inspect(string path)
        {
            using var image = new MagickImage(path);
            return (image.Width, image.Height, image.Format);
        }

        private static void AssertJpegMagic(string path)
        {
            using var stream = File.OpenRead(path);
            Assert.Equal(0xFF, stream.ReadByte());
            Assert.Equal(0xD8, stream.ReadByte());
            Assert.Equal(0xFF, stream.ReadByte());
        }

        [Fact]
        public async Task LandscapePngIsScaledToFitKeepingAspect()
        {
            var source = GeneratePng("landscape.png", 800, 600, transparent: false);
            var destination = Path.Combine(workDir, "landscape.jpg");

            await service.CreateThumbnailAsync(source, destination, 512, 512);

            AssertJpegMagic(destination);
            var (width, height, format) = Inspect(destination);
            Assert.Equal(MagickFormat.Jpeg, format);
            Assert.Equal(512u, width);
            Assert.Equal(384u, height);
        }

        [Fact]
        public async Task PortraitTransparentPngIsFlattenedAndFitsTheHeight()
        {
            var source = GeneratePng("portrait.png", 300, 900, transparent: true);
            var destination = Path.Combine(workDir, "nested", "portrait.jpg");

            await service.CreateThumbnailAsync(source, destination, 512, 512);

            AssertJpegMagic(destination);
            var (width, height, _) = Inspect(destination);
            Assert.Equal(512u, height);
            Assert.InRange(width, 170u, 171u);
            using var image = new MagickImage(destination);
            var pixel = image.GetPixels().GetPixel(10, 10).ToColor();
            Assert.True(pixel.R > 240 && pixel.G > 240 && pixel.B > 240, $"transparent areas should flatten to white, got {pixel}");
        }

        [Fact]
        public async Task TwoSecondMp4GetsAFrameAtOneSecond()
        {
            var source = GenerateVideo("two-seconds.mp4", 2, "-c:v libx264 -pix_fmt yuv420p");
            var destination = Path.Combine(workDir, "two-seconds.jpg");

            await service.CreateThumbnailAsync(source, destination, 512, 512);

            AssertJpegMagic(destination);
            var (width, height, _) = Inspect(destination);
            Assert.Equal(512u, width);
            Assert.Equal(384u, height);
        }

        [Fact]
        public async Task ClipShorterThanOneSecondUsesTheFirstFrame()
        {
            var source = GenerateVideo("short.mp4", 0.3, "-c:v libx264 -pix_fmt yuv420p");
            var destination = Path.Combine(workDir, "short.jpg");

            await service.CreateThumbnailAsync(source, destination, 256, 256);

            AssertJpegMagic(destination);
            var (width, height, _) = Inspect(destination);
            Assert.Equal(256u, width);
            Assert.Equal(192u, height);
        }

        [Fact]
        public async Task GifGoesThroughFfmpeg()
        {
            var source = GenerateVideo("anim.gif", 2);
            var destination = Path.Combine(workDir, "anim.jpg");

            await service.CreateThumbnailAsync(source, destination, 512, 512);

            AssertJpegMagic(destination);
            var (width, height, _) = Inspect(destination);
            Assert.Equal(512u, width);
            Assert.Equal(384u, height);
        }

        [Fact]
        public async Task CorruptVideoReportsFfmpegError()
        {
            var source = Path.Combine(workDir, "broken.mp4");
            await File.WriteAllBytesAsync(source, new byte[] { 1, 2, 3, 4, 5, 6, 7, 8 });
            var destination = Path.Combine(workDir, "broken.jpg");

            var ex = await Assert.ThrowsAsync<InvalidOperationException>(() => service.CreateThumbnailAsync(source, destination, 512, 512));

            Assert.Contains("ffmpeg", ex.Message);
            Assert.Contains("broken.mp4", ex.Message);
            Assert.False(File.Exists(destination));
        }

        [Fact]
        public async Task UnsupportedExtensionIsRejected()
        {
            var source = Path.Combine(workDir, "notes.txt");
            await File.WriteAllTextAsync(source, "hello");
            await Assert.ThrowsAsync<NotSupportedException>(() => service.CreateThumbnailAsync(source, Path.Combine(workDir, "notes.jpg"), 512, 512));
        }

        [Fact]
        public async Task MissingSourceIsRejected()
        {
            await Assert.ThrowsAsync<FileNotFoundException>(() => service.CreateThumbnailAsync(Path.Combine(workDir, "missing.png"), Path.Combine(workDir, "missing.jpg"), 512, 512));
        }
    }
}
