using Lively.Common.Helpers.Shell;
using System.Drawing.Imaging;
using System.Threading.Tasks;

namespace Lively.Common.Services
{
    public class WindowsThumbnailService : IThumbnailService
    {
        public Task CreateThumbnailAsync(string sourcePath, string destinationJpegPath, int width, int height)
        {
            using var thumbnail = ThumbnailUtil.GetThumbnail(sourcePath, width, height, ThumbnailUtil.ThumbnailOptions.None);
            thumbnail.Save(destinationJpegPath, ImageFormat.Jpeg);
            return Task.CompletedTask;
        }
    }
}
