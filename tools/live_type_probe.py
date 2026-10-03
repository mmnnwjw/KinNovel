#!/usr/bin/env python3
"""检查各列表接口用什么字段区分小说/漫画(只读, 低请求频率)。"""

import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin" / "src"))
sys.path.insert(0, str(ROOT / "bin" / "vendor"))

from kinnovel.api import ApiClient  # noqa: E402
from kinnovel.config import Config  # noqa: E402
from kinnovel.api import is_comic  # noqa: E402


def distribution(items, keys=("Type", "type")):
    counts = {}
    for item in items or []:
        if not isinstance(item, dict):
            counts["<non-dict>"] = counts.get("<non-dict>", 0) + 1
            continue
        found = None
        for key in keys:
            if key in item:
                found = "%s=%r" % (key, item[key])
                break
        counts[found or "<no-type-field>"] = counts.get(found or "<no-type-field>", 0) + 1
    return counts


def main():
    config = Config()
    email = str(config.get("account_email") or "").strip()
    password = str(config.get("account_password") or "")
    if not email or not password:
        print("config.json 未配置账号")
        return 2
    api = ApiClient(config)
    api.login(email, password)

    ranked = api.hub.invoke("GetRank", {"Days": 1})
    print("[rank] n=%s %s" % (len(ranked or []), distribution(ranked)))
    if ranked:
        print("[rank] sample keys=%s" % sorted(ranked[0].keys()))
    filtered_rank = api.get_rank(1)
    print("[rank] filtered n=%s comic=%s" % (
        len(filtered_rank), sum(1 for i in filtered_rank if is_comic(i))))

    listing = api.hub.invoke("GetBookList", {
        "Page": 1, "Size": 10, "Order": "latest",
        "IgnoreJapanese": False, "IgnoreAI": False})
    data = listing.get("Data") if isinstance(listing, dict) else listing
    print("[browse] n=%s %s" % (len(data or []), distribution(data)))
    filtered_list = api.get_book_list(page=1, size=10)
    fdata = filtered_list.get("Data") if isinstance(filtered_list, dict) else filtered_list
    print("[browse] filtered n=%s comic=%s" % (
        len(fdata or []), sum(1 for i in (fdata or []) if is_comic(i))))

    shelf = api.hub.invoke("GetBookShelf", {})
    shelf_items = shelf.get("data") if isinstance(shelf, dict) else shelf
    print("[shelf] n=%s %s" % (
        len(shelf_items or []), distribution(shelf_items)))
    if shelf_items:
        print("[shelf] sample keys=%s" % sorted(shelf_items[0].keys()))
    filtered_shelf = api.get_book_shelf()
    fshelf = filtered_shelf.get("data") if isinstance(filtered_shelf, dict) else filtered_shelf
    print("[shelf] filtered n=%s comic=%s" % (
        len(fshelf or []),
        sum(1 for i in (fshelf or []) if str(i.get("type") or "").lower() == "comic")))

    history = api.hub.invoke("GetReadHistory", {})
    print("[history] keys=%s" % (
        sorted(history.keys()) if isinstance(history, dict) else type(history)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
