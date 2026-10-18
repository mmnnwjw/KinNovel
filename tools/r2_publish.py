"""把一个 GitHub 发行版同步到自己的更新服务器 (Cloudflare R2 桶 kinnovel-updates, 公开域名 update.mmnnwjw.dpdns.org)。

上传两样东西 (客户端 `rust/crates/kn-app/src/update.rs` 读取):
- `KinNovel-vX.Y.Z.zip`: 安装包, 文件名带版本号, 内容不会再变, 允许长期缓存。
- `releases.json`: 最近 20 个发行版, 格式与 GitHub `GET /repos/{repo}/releases` 相同 (只留客户端用到的字段),
  资产的 `digest` = "sha256:…"。文件名固定, 上传时设 `Cache-Control: no-cache`。
先传安装包, 再传 releases.json, 所以列表里的版本在服务器上一定已经有包。
安装包必须与 GitHub 资产的 digest 一致 (没有 digest 时用本地算出的)。上传后经公开域名读回来校验。

用法:
  CI:   python tools/r2_publish.py v1.6.3 --zip build/KinNovel-v1.6.3.zip
        环境变量 R2_ENDPOINT / R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY, GH_TOKEN (可选, 读 GitHub API)
  本地: python tools/r2_publish.py v1.6.2 --local-secrets   (没给 --zip 时从 GitHub 下载这个版本的资产)
        凭据从 ~/.kinovel-secrets/{S3_API,Access_Key_ID,Secret_Access_Key,GithubToken} 读, 不打印。
"""
import argparse
import hashlib
import json
import os
import sys
import urllib.request

import boto3
from botocore.config import Config

REPO = 'mmnnwjw/KinNovel'
BUCKET = 'kinnovel-updates'
PUBLIC = 'https://update.mmnnwjw.dpdns.org'
# Cloudflare 会拦 Python-urllib 默认的 User-Agent (403)
UA = 'KinNovel-release/1.0'
KEEP_FIELDS = ('tag_name', 'name', 'published_at', 'created_at', 'prerelease', 'draft', 'body')


def http_get(url, token=None):
    headers = {'User-Agent': UA}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=120) as r:
        return r.read()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('tag')
    ap.add_argument('--zip')
    ap.add_argument('--local-secrets', action='store_true')
    args = ap.parse_args()
    tag = args.tag
    name = f'KinNovel-{tag}.zip'

    if args.local_secrets:
        d = os.path.expanduser('~/.kinovel-secrets')
        rd = lambda n: open(os.path.join(d, n), encoding='utf-8').read().strip()
        endpoint, key_id, secret, token = rd('S3_API'), rd('Access_Key_ID'), rd('Secret_Access_Key'), rd('GithubToken')
    else:
        endpoint, key_id, secret = (os.environ[k] for k in ('R2_ENDPOINT', 'R2_ACCESS_KEY_ID', 'R2_SECRET_ACCESS_KEY'))
        token = os.environ.get('GH_TOKEN')

    releases = json.loads(http_get(f'https://api.github.com/repos/{REPO}/releases?per_page=20', token))
    releases = [r for r in releases if not r.get('draft')]
    current = next((r for r in releases if r['tag_name'] == tag), None)
    if current is None:
        sys.exit(f'GitHub 上没有发行版 {tag}')
    asset = next((a for a in current.get('assets', []) if a['name'] == name), None)
    if asset is None:
        sys.exit(f'{tag} 没有资产 {name}')

    if args.zip:
        data = open(args.zip, 'rb').read()
    else:
        data = http_get(asset['browser_download_url'])
    digest = hashlib.sha256(data).hexdigest()
    expected = (asset.get('digest') or '').removeprefix('sha256:')
    if expected and expected != digest:
        sys.exit(f'{name}: sha256 {digest} 与 GitHub 资产 {expected} 不一致')
    if len(data) != asset['size']:
        sys.exit(f'{name}: 大小 {len(data)} 与 GitHub 资产 {asset["size"]} 不一致')

    listing = []
    for r in releases:
        item = {k: r.get(k) for k in KEEP_FIELDS}
        item['assets'] = [
            {'name': a['name'], 'size': a['size'], 'digest': a.get('digest') or (f'sha256:{digest}' if a is asset else None)}
            for a in r.get('assets', []) if a['name'].endswith('.zip')
        ]
        listing.append(item)
    body = json.dumps(listing, ensure_ascii=False, indent=1).encode('utf-8')

    s3 = boto3.client('s3', endpoint_url=endpoint, aws_access_key_id=key_id, aws_secret_access_key=secret,
                      region_name='auto', config=Config(signature_version='s3v4'))
    s3.put_object(Bucket=BUCKET, Key=name, Body=data, ContentType='application/zip',
                  CacheControl='public, max-age=31536000, immutable')
    print(f'uploaded {name}: {len(data)} B, sha256 {digest}')
    s3.put_object(Bucket=BUCKET, Key='releases.json', Body=body, ContentType='application/json; charset=utf-8',
                  CacheControl='no-cache')
    print(f'uploaded releases.json: {len(listing)} releases, {len(body)} B')

    # 经公开域名读回来
    got = http_get(f'{PUBLIC}/{name}')
    if hashlib.sha256(got).hexdigest() != digest:
        sys.exit(f'{PUBLIC}/{name} 读回来的内容不一致')
    got = json.loads(http_get(f'{PUBLIC}/releases.json?t={digest[:8]}'))
    if not any(r['tag_name'] == tag for r in got):
        sys.exit(f'{PUBLIC}/releases.json 里没有 {tag}')
    print(f'public check OK: {PUBLIC}/{name}, {PUBLIC}/releases.json')


if __name__ == '__main__':
    main()
