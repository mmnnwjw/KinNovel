#!/usr/bin/env python3
"""Probe two small catalogue pages with a ten-second timer between requests."""

import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin" / "src"))

from kinnovel.api import ApiClient, SessionStore
from kinnovel.config import Config
from kinnovel.utils import sha256_text


DELAY = 10


def request(name, operation):
    print("[request]", name, flush=True)
    result = operation()
    print("[ok]", name, flush=True)
    time.sleep(DELAY)
    return result


def main():
    lines = [
        line.strip()
        for line in (ROOT / "TESTACCOUNT.txt").read_text(
            encoding="utf-8-sig").splitlines()
        if line.strip()
    ]
    session_path = ROOT / "build" / "browse-paging-session.json"
    try:
        session_path.unlink()
    except OSError:
        pass
    api = ApiClient(Config(), SessionStore(session_path))
    try:
        credentials = request(
            "login",
            lambda: api._http("/api/user/login", {
                "email": lines[0],
                "password": sha256_text(lines[1]),
            }),
        )
        api._store_credentials(credentials)
        api.session.set_many({"TokenUpdatedAt": time.time() + 3600})
        first = request(
            "GetBookList page=1 size=12",
            lambda: api.get_book_list(page=1, size=12, order="latest"),
        )
        second = request(
            "GetBookList page=2 size=12",
            lambda: api.get_book_list(page=2, size=12, order="latest"),
        )
        first_ids = [item.get("Id") for item in (first.get("Data") or [])]
        second_ids = [item.get("Id") for item in (second.get("Data") or [])]
        print("[summary] first", len(first_ids), first.get("Page"),
              first.get("TotalPages"), first_ids[:3], flush=True)
        print("[summary] second", len(second_ids), second.get("Page"),
              second.get("TotalPages"), second_ids[:3], flush=True)
        print("[summary] overlap", len(set(first_ids) & set(second_ids)), flush=True)
    finally:
        try:
            api.hub.close()
        except Exception:
            pass
        try:
            session_path.unlink()
        except OSError:
            pass


if __name__ == "__main__":
    main()
