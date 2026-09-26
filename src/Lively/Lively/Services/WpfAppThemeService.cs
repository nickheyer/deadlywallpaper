using Lively.Common.Services;
using Lively.Models.Enums;

namespace Lively.Services
{
    public class WpfAppThemeService : IAppThemeService
    {
        public void ChangeTheme(AppTheme theme) => App.ChangeTheme(theme);
    }
}
