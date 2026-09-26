using Lively.Common.Services;

namespace Lively.Services
{
    public class WpfAppLifetimeService : IAppLifetimeService
    {
        public void Quit() => App.QuitApp();
    }
}
