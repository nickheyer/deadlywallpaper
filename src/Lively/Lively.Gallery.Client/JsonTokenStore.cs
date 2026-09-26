using Lively.Gallery.Client.Interfaces;
using System;
using System.Text;
using static Lively.Common.Constants;
using Lively.Common.Helpers.Storage;
using Lively.Common.Services;
using Lively.Models.Gallery.API;
using Newtonsoft.Json;

namespace Lively.Gallery.Client
{
    public class JsonTokenStore : ITokenStore
    {
        private readonly ITokenProtector protector;
        private TokensModel _tokens;

        public JsonTokenStore(ITokenProtector protector)
        {
            this.protector = protector;
        }

        public void Clear()
        {
            _tokens = new();
            try
            {
                Store(_tokens, CommonPaths.TokensPath);
            }
            catch { }
        }

        public TokensModel Get()
        {
            if (_tokens == null)
            {
                try
                {
                    _tokens = Load<TokensModel>(CommonPaths.TokensPath);
                    //Debug.WriteLine($"Accesstoken:{_tokens?.AccessToken}");
                }
                catch { }
            }
            return _tokens;
        }

        public void Set(string accessToken, string refreshToken, string provider, DateTime expiration)
        {
            _tokens = new()
            {
                AccessToken = accessToken,
                RefreshToken = refreshToken,
                Expiration = expiration,
                Provider = provider
            };
     
            try
            {
                Store(_tokens, CommonPaths.TokensPath);
            }
            catch { }
        }

        private void Store<T>(T data, string filePath) =>
            JsonStorage<byte[]>.StoreData(filePath, protector.Protect(Encoding.UTF8.GetBytes(JsonConvert.SerializeObject(data))));

        private T Load<T>(string filePath) =>
            JsonConvert.DeserializeObject<T>(Encoding.UTF8.GetString(protector.Unprotect(JsonStorage<byte[]>.LoadData(filePath))));
    }
}
