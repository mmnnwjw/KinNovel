"""Gitee 镜像 (国内连不上 GitHub 时程序从这里检查更新 / 下载安装包)。CI 调用, 只用标准库。

  python tools/gitee_release.py ensure-repo
      仓库 gitee.com/<GITEE_USER>/KinNovel 不存在时创建 (公开)。
  python tools/gitee_release.py wait-tag <tag> [分钟]
      等标签出现在 Gitee 上 (由 gitee-sync.yml 推送), 默认最多等 40 分钟。
  python tools/gitee_release.py release <tag> <zip> [notes.md]
      创建 (或更新) 该标签的发行版: 说明 = notes.md + "SHA256: <zip 的哈希>" (程序用它校验下载),
      附件 = zip (同名旧附件先删掉)。标签要先在 Gitee 上 (wait-tag)。

环境变量 GITEE_USER / GITEE_TOKEN (仓库 Secrets)。没设置时什么也不做 (退出码 0), 不影响 GitHub 发布。
"""

import hashlib
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

API = "https://gitee.com/api/v5"
REPO = "KinNovel"


def call(method, path, token, fields=None, files=None):
    url = f"{API}{path}"
    fields = dict(fields or {}, access_token=token)
    if method in ("GET", "DELETE"):
        url += "?" + urllib.parse.urlencode(fields)
        data, headers = None, {}
    elif files:
        boundary = uuid.uuid4().hex
        parts = []
        for k, v in fields.items():
            parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode())
        for k, (name, blob) in files.items():
            parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"; filename="{name}"\r\n'
                         f"Content-Type: application/zip\r\n\r\n".encode() + blob + b"\r\n")
        parts.append(f"--{boundary}--\r\n".encode())
        data = b"".join(parts)
        headers = {"Content-Type": f"multipart/form-data; boundary={boundary}"}
    else:
        data = json.dumps(fields).encode()
        headers = {"Content-Type": "application/json;charset=UTF-8"}
    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            body = r.read()
            return r.status, (json.loads(body) if body.strip() else None)
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", "replace")[:500]


def ensure_repo(user, token):
    status, _ = call("GET", f"/repos/{user}/{REPO}", token)
    if status == 200:
        print(f"gitee: {user}/{REPO} exists")
        return
    status, body = call("POST", "/user/repos", token, {
        "name": REPO, "path": REPO, "private": False, "has_issues": False, "has_wiki": False,
        "description": "KinNovel 的 Gitee 镜像 (由 GitHub Actions 同步; 主仓库 github.com/mmnnwjw/KinNovel)",
    })
    print(f"gitee: create {user}/{REPO} -> {status} {body if status >= 300 else ''}")
    if status >= 300:
        sys.exit(1)


def wait_tag(user, token, tag, minutes=40):
    deadline = time.time() + minutes * 60
    while True:
        status, body = call("GET", f"/repos/{user}/{REPO}/tags", token, {"per_page": 100})
        names = [t.get("name") for t in body] if status == 200 and isinstance(body, list) else []
        if tag in names:
            print(f"gitee: tag {tag} is there")
            return
        if time.time() > deadline:
            print(f"gitee: tag {tag} not mirrored after {minutes} min (status {status}); check the pull mirror")
            sys.exit(1)
        print(f"gitee: waiting for {tag} ({len(names)} tags so far)")
        time.sleep(30)


def release(user, token, tag, zip_path, notes_path=None):
    blob = open(zip_path, "rb").read()
    digest = hashlib.sha256(blob).hexdigest()
    notes = open(notes_path, encoding="utf-8").read().strip() if notes_path and os.path.exists(notes_path) else ""
    body = f"{notes}\n\nSHA256: {digest}\n".lstrip()
    name = os.path.basename(zip_path)
    repo = f"/repos/{user}/{REPO}"
    status, rel = call("GET", f"{repo}/releases/tags/{tag}", token)
    if status == 200 and rel and rel.get("id"):
        status, rel = call("PATCH", f"{repo}/releases/{rel['id']}", token, {"tag_name": tag, "name": f"KinNovel {tag[1:]}", "body": body})
    else:
        status, rel = call("POST", f"{repo}/releases", token, {
            "tag_name": tag, "name": f"KinNovel {tag[1:]}", "body": body, "target_commitish": "main", "prerelease": False,
        })
    if status >= 300 or not isinstance(rel, dict):
        print(f"gitee: release {tag} -> {status} {rel}")
        sys.exit(1)
    rid = rel["id"]
    _, files = call("GET", f"{repo}/releases/{rid}/attach_files", token)
    for f in files if isinstance(files, list) else []:
        if f.get("name") == name:
            call("DELETE", f"{repo}/releases/{rid}/attach_files/{f['id']}", token)
    status, up = call("POST", f"{repo}/releases/{rid}/attach_files", token, files={"file": (name, blob)})
    print(f"gitee: release {tag} id={rid}, upload {name} ({len(blob)} B, sha256 {digest}) -> {status}")
    if status >= 300:
        print(up)
        sys.exit(1)


def main():
    user, token = os.environ.get("GITEE_USER", "").strip(), os.environ.get("GITEE_TOKEN", "").strip()
    if not user or not token:
        print("gitee: GITEE_USER / GITEE_TOKEN not set, skipping")
        return
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "ensure-repo":
        ensure_repo(user, token)
    elif cmd == "wait-tag" and len(sys.argv) >= 3:
        wait_tag(user, token, sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 40)
    elif cmd == "release" and len(sys.argv) >= 4:
        release(user, token, sys.argv[2], sys.argv[3], sys.argv[4] if len(sys.argv) > 4 else None)
    else:
        print(__doc__)
        sys.exit(2)


if __name__ == "__main__":
    main()
