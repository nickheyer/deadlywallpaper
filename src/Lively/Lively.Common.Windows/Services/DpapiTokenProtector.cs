using Lively.Common.Helpers;

namespace Lively.Common.Services
{
    public class DpapiTokenProtector : ITokenProtector
    {
        public byte[] Protect(byte[] data) => EncryptUtil.Protect(data);

        public byte[] Unprotect(byte[] data) => EncryptUtil.Unprotect(data);
    }
}
