"""给发布包签名: 写 `KinNovel-vX.Y.Z.zip.sig` (Ed25519 签名的十六进制, 一行)。

签名内容 = "kinnovel-update-v1\\n<包名>\\n<sha256 十六进制>\\n", 与客户端 `rust/crates/kn-app/src/update.rs`
`signed_message` 一致; 客户端内置公钥, 验签不过的包不装。

私钥 (32 字节种子的十六进制):
  CI:   环境变量 UPDATE_SIGNING_KEY (仓库 Secret)
  本地: ~/.kinovel-secrets/UPDATE_SIGNING_KEY (--local-secrets), 不打印
用法: python tools/sign_update.py build/KinNovel-v1.6.3.zip [--local-secrets]
"""
import argparse
import hashlib
import os
import sys

from nacl.signing import SigningKey

PUBLIC_KEY = '579c991f5684ee31379fdaaed587de2a0ee1147c7e8ee2d757bb03fc2008d9df'


def message(package, sha256):
    return f'kinnovel-update-v1\n{package}\n{sha256}\n'.encode()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('zip')
    ap.add_argument('--local-secrets', action='store_true')
    args = ap.parse_args()
    if args.local_secrets:
        seed = open(os.path.expanduser('~/.kinovel-secrets/UPDATE_SIGNING_KEY'), encoding='utf-8').read().strip()
    else:
        seed = os.environ.get('UPDATE_SIGNING_KEY', '').strip()
    if not seed:
        sys.exit('没有签名私钥 (UPDATE_SIGNING_KEY)')
    key = SigningKey(bytes.fromhex(seed))
    if key.verify_key.encode().hex() != PUBLIC_KEY:
        sys.exit('私钥与客户端内置的公钥不配对')
    package = os.path.basename(args.zip)
    sha256 = hashlib.sha256(open(args.zip, 'rb').read()).hexdigest()
    sig = key.sign(message(package, sha256)).signature.hex()
    open(args.zip + '.sig', 'w', newline='').write(sig + '\n')
    print(f'signed {package} (sha256 {sha256})')


if __name__ == '__main__':
    main()
