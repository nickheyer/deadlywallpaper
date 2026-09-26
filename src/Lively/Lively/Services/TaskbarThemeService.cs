using Lively.Common.Services;
using Lively.Models.Enums;

namespace Lively.Services
{
    public class TaskbarThemeService : ITaskbarThemeService
    {
        private readonly ITransparentTbService ttbService;

        public TaskbarThemeService(ITransparentTbService ttbService)
        {
            this.ttbService = ttbService;
        }

        public void Apply(TaskbarTheme theme) => ttbService.Start(theme);
    }
}
