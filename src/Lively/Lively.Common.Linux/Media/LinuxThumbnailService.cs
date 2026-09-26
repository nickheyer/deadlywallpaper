using ImageMagick;
using Lively.Common.Services;
using Lively.Models.Enums;
using NLog;
using System;
using System.Diagnostics;
using System.IO;
using System.Threading.Tasks;

namespace Lively.Common.Linux.Media
{
    /// <summary>
    /// Creates JPEG thumbnails: images through Magick.NET, videos and gifs by grabbing a frame with <c>ffmpeg</c>.
    /// </summary>
    public sealed class LinuxThumbnailService : IThumbnailService
    {
        private static readonly Logger logger = LogManager.GetCurrentClassLogger();
        private const int JpegQuality = 90;
        private static readonly TimeSpan FfmpegTimeout = TimeSpan.FromMinutes(2);

        public async Task CreateThumbnailAsync(string sourcePath, string destinationJpegPath, int width, int height)
        {
            if (string.IsNullOrEmpty(sourcePath))
                throw new ArgumentException("Source path is required.", nameof(sourcePath));
            if (string.IsNullOrEmpty(destinationJpegPath))
                throw new ArgumentException("Destination path is required.", nameof(destinationJpegPath));
            if (width <= 0)
                throw new ArgumentOutOfRangeException(nameof(width), width, "Width must be positive.");
            if (height <= 0)
                throw new ArgumentOutOfRangeException(nameof(height), height, "Height must be positive.");
            if (!File.Exists(sourcePath))
                throw new FileNotFoundException("Thumbnail source does not exist.", sourcePath);

            var destinationDirectory = Path.GetDirectoryName(Path.GetFullPath(destinationJpegPath));
            if (!string.IsNullOrEmpty(destinationDirectory))
                Directory.CreateDirectory(destinationDirectory);

            switch (FileTypes.GetFileType(sourcePath))
            {
                case WallpaperType.picture:
                    await Task.Run(() => CreateImageThumbnail(sourcePath, destinationJpegPath, width, height)).ConfigureAwait(false);
                    break;
                case WallpaperType.video:
                case WallpaperType.gif:
                    await CreateVideoThumbnailAsync(sourcePath, destinationJpegPath, width, height).ConfigureAwait(false);
                    break;
                default:
                    throw new NotSupportedException($"Cannot create a thumbnail for '{sourcePath}': '{Path.GetExtension(sourcePath)}' is not a supported image, video or gif extension.");
            }
        }

        private static void CreateImageThumbnail(string sourcePath, string destinationJpegPath, int width, int height)
        {
            using var image = new MagickImage(sourcePath);
            image.AutoOrient();
            if (image.ColorSpace == ColorSpace.CMYK)
                image.ColorSpace = ColorSpace.sRGB;
            if (image.HasAlpha)
            {
                image.BackgroundColor = MagickColors.White;
                image.Alpha(AlphaOption.Remove);
            }
            image.Resize(new MagickGeometry((uint)width, (uint)height));
            image.Format = MagickFormat.Jpeg;
            image.Quality = JpegQuality;
            image.Write(destinationJpegPath);
        }

        private static async Task CreateVideoThumbnailAsync(string sourcePath, string destinationJpegPath, int width, int height)
        {
            var (exitCode, stderr) = await RunFfmpegAsync(sourcePath, destinationJpegPath, width, height, seekSeconds: 1).ConfigureAwait(false);
            if (ProducedOutput(destinationJpegPath))
                return;

            logger.Debug("ffmpeg produced no frame at 1s for {0} (exit code {1}: {2}); retrying from the first frame.", sourcePath, exitCode, stderr);
            DeleteIfEmpty(destinationJpegPath);
            (exitCode, stderr) = await RunFfmpegAsync(sourcePath, destinationJpegPath, width, height, seekSeconds: null).ConfigureAwait(false);
            if (ProducedOutput(destinationJpegPath))
                return;

            DeleteIfEmpty(destinationJpegPath);
            throw new InvalidOperationException($"ffmpeg could not create a thumbnail for '{sourcePath}' (exit code {exitCode}): {stderr}");
        }

        private static async Task<(int exitCode, string stderr)> RunFfmpegAsync(string sourcePath, string destinationJpegPath, int width, int height, int? seekSeconds)
        {
            var psi = new ProcessStartInfo("ffmpeg")
            {
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                CreateNoWindow = true,
            };
            psi.ArgumentList.Add("-y");
            psi.ArgumentList.Add("-hide_banner");
            psi.ArgumentList.Add("-loglevel");
            psi.ArgumentList.Add("error");
            if (seekSeconds.HasValue)
            {
                psi.ArgumentList.Add("-ss");
                psi.ArgumentList.Add(seekSeconds.Value.ToString(System.Globalization.CultureInfo.InvariantCulture));
            }
            psi.ArgumentList.Add("-i");
            psi.ArgumentList.Add(sourcePath);
            psi.ArgumentList.Add("-frames:v");
            psi.ArgumentList.Add("1");
            psi.ArgumentList.Add("-vf");
            psi.ArgumentList.Add($"scale={width}:{height}:force_original_aspect_ratio=decrease");
            psi.ArgumentList.Add("-q:v");
            psi.ArgumentList.Add("3");
            psi.ArgumentList.Add(destinationJpegPath);

            using var process = Process.Start(psi)
                ?? throw new InvalidOperationException("Failed to start ffmpeg.");
            var stdoutTask = process.StandardOutput.ReadToEndAsync();
            var stderrTask = process.StandardError.ReadToEndAsync();
            using var timeout = new System.Threading.CancellationTokenSource(FfmpegTimeout);
            try
            {
                await process.WaitForExitAsync(timeout.Token).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                process.Kill(entireProcessTree: true);
                throw new TimeoutException($"ffmpeg did not finish within {FfmpegTimeout.TotalSeconds:0} s while thumbnailing '{sourcePath}'.");
            }
            await stdoutTask.ConfigureAwait(false);
            var stderr = (await stderrTask.ConfigureAwait(false)).Trim();
            return (process.ExitCode, stderr);
        }

        private static bool ProducedOutput(string path) => File.Exists(path) && new FileInfo(path).Length > 0;

        private static void DeleteIfEmpty(string path)
        {
            if (File.Exists(path) && new FileInfo(path).Length == 0)
                File.Delete(path);
        }
    }
}
