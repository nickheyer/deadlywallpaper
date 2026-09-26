namespace Lively.Common.Services
{
    public interface ITokenProtector
    {
        byte[] Protect(byte[] data);
        byte[] Unprotect(byte[] data);
    }
}
