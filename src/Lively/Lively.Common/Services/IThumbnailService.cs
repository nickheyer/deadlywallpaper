using System.Threading.Tasks;

namespace Lively.Common.Services
{
    public interface IThumbnailService
    {
        Task CreateThumbnailAsync(string sourcePath, string destinationJpegPath, int width, int height);
    }
}
