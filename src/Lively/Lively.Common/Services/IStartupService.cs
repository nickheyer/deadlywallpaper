using System.Threading.Tasks;

namespace Lively.Common.Services
{
    public interface IStartupService
    {
        Task<bool> TrySetStartupAsync(bool enabled);
    }
}
