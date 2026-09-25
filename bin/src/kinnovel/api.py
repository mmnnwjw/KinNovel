import json
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

from .config import APP_DIR, Config
from .transport import ApiError, SignalRClient, TransportError
from .utils import atomic_write, read_json, sha256_text


SESSION_PATH = APP_DIR / "cache" / "session.json"


class SessionStore:
    def __init__(self, path=SESSION_PATH):
        self.path = path
        self._lock = threading.RLock()
        self.data = read_json(path, {}) or {}

    def save(self):
        with self._lock:
            atomic_write(self.path, json.dumps(self.data, ensure_ascii=False, indent=2))

    def get(self, key, default=None):
        with self._lock:
            return self.data.get(key, default)

    def set_many(self, values):
        with self._lock:
            self.data.update(values)
            self.save()

    def clear_credentials(self):
        with self._lock:
            self.data.pop("Token", None)
            self.data.pop("RefreshToken", None)
            self.data.pop("TokenUpdatedAt", None)
            self.data.pop("User", None)
            self.save()


class ApiClient:
    def __init__(self, config=None, session=None):
        self.config = config or Config()
        self.session = session or SessionStore()
        self._refresh_lock = threading.Lock()
        self._request_lock = threading.RLock()
        self.server = str(self.config.get("api_server") or "").rstrip("/")
        visitor_path = APP_DIR / "cache" / "visitor-id"
        try:
            visitor_id = visitor_path.read_text(encoding="ascii").strip()
        except OSError:
            visitor_id = ""
        if not visitor_id:
            visitor_id = __import__("uuid").uuid4().hex
            atomic_write(visitor_path, visitor_id)
        self.hub = SignalRClient(
            self.server,
            token_provider=self.get_access_token,
            strict_tls=bool(self.config.get("strict_tls")),
            request_limit=int(self.config.get("request_limit") or 9),
            request_window=float(self.config.get("request_window_ms") or 5500) / 1000.0,
            visitor_id=visitor_id,
        )

    def set_server(self, server):
        self.server = str(server or "").rstrip("/")
        self.config.set("api_server", self.server)
        self.hub.set_server(self.server)

    @property
    def user(self):
        return self.session.get("User")

    @property
    def user_id(self):
        user = self.user or {}
        return int(user.get("Id") or 0)

    def has_refresh_token(self):
        return bool(self.session.get("RefreshToken"))

    def _ssl_context(self):
        import ssl
        context = ssl.create_default_context()
        if not self.config.get("strict_tls"):
            context.check_hostname = False
            context.verify_mode = ssl.CERT_NONE
        return context

    def _http(self, path, payload=None, method="POST", token=None, timeout=30):
        url = path if str(path).startswith("http") else self.server + path
        headers = {
            "Accept": "application/json",
            "User-Agent": "KinNovel/0.1",
            "x-id": getattr(self.hub, "_visitor_id", "kinnovel"),
        }
        data = None
        if method == "GET":
            if payload:
                query = urllib.parse.urlencode(payload)
                url += ("&" if "?" in url else "?") + query
        elif payload is not None:
            data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
            headers["Content-Type"] = "application/json"
        if token:
            headers["Authorization"] = "Bearer " + token
        request = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(request, timeout=timeout,
                                        context=self._ssl_context()) as response:
                body = response.read()
                status = response.status
        except urllib.error.HTTPError as exc:
            body = exc.read()
            status = exc.code
        except OSError as exc:
            raise TransportError("网络错误: %s" % exc)
        try:
            content = json.loads(body.decode("utf-8"))
        except (UnicodeDecodeError, ValueError):
            if status >= 400:
                raise ApiError("HTTP %s" % status, status)
            raise TransportError("响应不是 JSON")
        success = content.get("Success", content.get("success"))
        if status >= 400 or success is False:
            message = content.get("Msg") or content.get("msg") or ("HTTP %s" % status)
            raise ApiError(str(message), int(content.get("Status") or content.get("status") or status))
        return content.get("Response", content.get("response"))

    def send_register_email(self, email):
        return self._http("/api/user/send_register_email", {"email": email}, "GET")

    def send_reset_email(self, email):
        return self._http("/api/user/send_reset_email", {"email": email}, "GET")

    def login(self, email, password):
        credentials = self._http("/api/user/login", {
            "email": email,
            "password": sha256_text(password),
        })
        self._store_credentials(credentials)
        user = self.get_my_info()
        self.session.set_many({"User": user})
        return user

    def register(self, username, email, password, code, invite_code):
        credentials = self._http("/api/user/register", {
            "userName": username,
            "email": email,
            "password": sha256_text(password),
            "code": code,
            "inviteCode": invite_code,
        })
        self._store_credentials(credentials)
        user = self.get_my_info()
        self.session.set_many({"User": user})
        return user

    def reset_password(self, email, new_password, code):
        return self._http("/api/user/reset_password", {
            "email": email,
            "newPassword": sha256_text(new_password),
            "code": code,
        })

    def _store_credentials(self, credentials):
        if not isinstance(credentials, dict):
            raise ApiError("登录响应缺少凭据")
        token = credentials.get("Token") or credentials.get("token")
        refresh = credentials.get("RefreshToken") or credentials.get("refreshToken")
        if not token or not refresh:
            raise ApiError("登录响应缺少 Token 或 RefreshToken")
        self.session.set_many({
            "Token": token,
            "RefreshToken": refresh,
            "TokenUpdatedAt": time.time(),
        })

    def refresh_access_token(self):
        refresh = self.session.get("RefreshToken")
        if not refresh:
            return None
        with self._refresh_lock:
            token = self.session.get("Token")
            updated = float(self.session.get("TokenUpdatedAt") or 0)
            if token and time.time() - updated < 25:
                return token
            try:
                token = self._http("/api/user/refresh_token", {"token": refresh})
            except ApiError as exc:
                if int(getattr(exc, "status", 500)) in (-100, 404):
                    self.session.clear_credentials()
                raise
            if not token:
                return None
            self.session.set_many({"Token": token, "TokenUpdatedAt": time.time()})
            return token

    def get_access_token(self):
        token = self.session.get("Token")
        updated = float(self.session.get("TokenUpdatedAt") or 0)
        if token and time.time() - updated < 25:
            return token
        try:
            return self.refresh_access_token()
        except Exception:
            return None

    def refresh_user(self):
        if not self.has_refresh_token():
            return None
        user = self.get_my_info()
        self.session.set_many({"User": user})
        return user

    def logout(self):
        self.session.clear_credentials()
        self.hub.close()

    def invoke(self, method, params=None):
        try:
            return self.hub.invoke(method, params or {})
        except ApiError as exc:
            if int(getattr(exc, "status", 500)) != 401:
                raise
            self.session.set_many({"Token": "", "TokenUpdatedAt": 0})
            if not self.refresh_access_token():
                raise
            return self.hub.invoke(method, params or {})

    # Public catalogue methods
    def get_latest_book_list(self, ignore_japanese=False, ignore_ai=False, page=1, size=6):
        return self.invoke("GetLatestBookList", {
            "Page": page,
            "Size": size,
            "IgnoreJapanese": bool(ignore_japanese),
            "IgnoreAI": bool(ignore_ai),
        })

    def get_book_list(self, page=1, size=12, keywords=None, order="latest",
                      ignore_japanese=False, ignore_ai=False, category_id=None):
        params = {
            "Page": int(page),
            "Size": int(size),
            "Order": order,
            "IgnoreJapanese": bool(ignore_japanese),
            "IgnoreAI": bool(ignore_ai),
        }
        if keywords is not None:
            params["KeyWords"] = keywords
        if category_id is not None:
            params["CategoryId"] = int(category_id)
        return self.invoke("GetBookList", params)

    def search_books(self, mode, keywords, page=1, size=12,
                     ignore_japanese=False, ignore_ai=False):
        params = {
            "Page": int(page),
            "Size": int(size),
            "KeyWords": keywords,
            "IgnoreJapanese": bool(ignore_japanese),
            "IgnoreAI": bool(ignore_ai),
        }
        method = {
            "title": "GetBookListByTitle",
            "author": "GetBookListByAuthor",
            "name": "GetBookListByName",
            "tags": "GetBookListByTags",
            "exact": "GetBookList",
            "fuzzy": "GetBookList",
        }.get(mode, "GetBookList")
        if mode == "exact":
            params["KeyWords"] = '"%s"' % keywords
        return self.invoke(method, params)

    def get_book_categories(self, book_type="Novel"):
        return self.invoke("GetBookCategories", {"Type": book_type})

    def get_series_list(self, page=1, size=12, order="latest", category_id=None,
                        ignore_japanese=False, ignore_ai=False):
        params = {
            "Page": int(page),
            "Size": int(size),
            "Order": order,
            "IgnoreJapanese": bool(ignore_japanese),
            "IgnoreAI": bool(ignore_ai),
        }
        if category_id is not None:
            params["CategoryId"] = int(category_id)
        return self.invoke("GetSeriesList", params)

    def get_books_by_series(self, series_name, page=1, size=12, order="latest"):
        return self.invoke("GetBooksBySeries", {
            "SeriesName": series_name,
            "Page": int(page),
            "Size": int(size),
            "Order": order,
        })

    def get_rank(self, days=1):
        return self.invoke("GetRank", {"Days": int(days)})

    def get_comic_list(self, page=1, size=12, order="latest"):
        return self.invoke("GetComicList", {"Page": int(page), "Size": int(size), "Order": order})

    def search_comic_series(self, keywords, mode="fuzzy", page=1, size=12,
                            ignore_japanese=False, ignore_ai=False):
        return self.invoke("SearchComicSeries", {
            "KeyWords": keywords,
            "Mode": mode,
            "Page": int(page),
            "Size": int(size),
            "IgnoreJapanese": bool(ignore_japanese),
            "IgnoreAI": bool(ignore_ai),
        })

    def get_online_info(self):
        return self.invoke("GetOnlineInfo")

    def get_announcement_list(self, page=1, size=12):
        return self.invoke("GetAnnouncementList", {"Page": int(page), "Size": int(size)})

    def get_announcement_detail(self, announcement_id):
        return self.invoke("GetAnnouncementDetail", {"Id": int(announcement_id)})

    def get_collaborator_list(self):
        return self.invoke("GetCollaboratorList")

    def get_ban_list(self):
        return self.invoke("GetBanList")

    # Authenticated catalogue and reading
    def get_book_info(self, book_id):
        return self.invoke("GetBookInfo", {"Id": int(book_id)})

    def get_book_list_by_ids(self, ids, book_type=None):
        if len(ids) > 24:
            raise ApiError("单次最多请求 24 本书", 400)
        params = {"Ids": [int(value) for value in ids]}
        if book_type:
            params["Type"] = book_type
        return self.invoke("GetBookListByIds", params)

    def get_novel_content(self, book_id, sort_num, convert=None):
        params = {"Bid": int(book_id), "SortNum": int(sort_num)}
        if convert:
            params["Convert"] = convert
        return self.invoke("GetNovelContent", params)

    def save_read_position(self, book_id, chapter_id, xpath):
        return self.invoke("SaveReadPosition", {
            "Bid": int(book_id),
            "Cid": int(chapter_id),
            "XPath": str(xpath or "."),
        })

    def get_read_position(self, book_id):
        return self.invoke("GetReadPosition", {"Id": int(book_id)})

    def get_read_history(self):
        return self.invoke("GetReadHistory")

    def clear_read_history(self):
        return self.invoke("ClearReadHistory")

    # User and shelf
    def get_my_info(self):
        return self.invoke("GetMyInfo")

    def get_public_user_summary(self, user_id):
        return self.invoke("GetUserSummary", {"UserId": int(user_id)})

    def get_notifications(self, page=1, size=12):
        return self.invoke("GetNotifications", {"Page": int(page), "Size": int(size)})

    def mark_notifications(self, ids):
        return self.invoke("MarkNotifications", {"Ids": [int(value) for value in ids]})

    def get_book_shelf(self):
        return self.invoke("GetBookShelf")

    def save_book_shelf(self, items, version="20260921"):
        return self.invoke("SaveBookShelf", {"data": items, "ver": version})

    def sign_in(self):
        return self.invoke("SignIn", {})

    def get_point_log(self, page=1, size=12):
        return self.invoke("GetPointLog", {"Page": int(page), "Size": int(size)})

    def get_coin_log(self, page=1, size=12):
        return self.invoke("GetCoinLog", {"Page": int(page), "Size": int(size)})

    def get_sign_in_calendar(self, year, month):
        return self.invoke("GetSignInCalendar", {"Year": int(year), "Month": int(month)})

    def get_shop(self):
        return self.invoke("GetShop", {})

    def get_my_items(self):
        return self.invoke("GetMyItems", {})

    def buy_shop_item(self, key, quantity=1):
        return self.invoke("BuyShopItem", {"Key": key, "Quantity": int(quantity)})

    def use_sign_makeup_card(self, date):
        return self.invoke("UseSignMakeupCard", {"Date": date})

    def use_comic_quota_card(self):
        return self.invoke("UseComicQuotaCard", {})

    # Comments
    def get_comments(self, comment_type, target_id, page=1):
        return self.invoke("GetComments", {
            "Type": comment_type,
            "Id": int(target_id),
            "Page": int(page),
        })

    def post_comment(self, comment_type, target_id, content):
        return self.invoke("PostComment", {
            "Type": comment_type,
            "Id": int(target_id),
            "Content": content,
        })

    def reply_comment(self, comment_type, target_id, content, reply_id, parent_id):
        return self.invoke("ReplyComment", {
            "Type": comment_type,
            "Id": int(target_id),
            "Content": content,
            "ReplyId": int(reply_id),
            "ParentId": int(parent_id),
        })

    def delete_comment(self, comment_id):
        return self.invoke("DeleteComment", {"Id": int(comment_id)})

    # Direct messages
    def get_direct_conversations(self, before_message_id=0, size=20):
        return self.invoke("GetDirectConversations", {
            "BeforeMessageId": int(before_message_id),
            "Size": int(size),
        })

    def get_direct_messages(self, peer_user_id, before_message_id=0, size=30):
        return self.invoke("GetDirectMessages", {
            "PeerUserId": int(peer_user_id),
            "BeforeMessageId": int(before_message_id),
            "Size": int(size),
        })

    def send_direct_message(self, recipient_user_id, client_message_id, content):
        return self.invoke("SendDirectMessage", {
            "RecipientUserId": int(recipient_user_id),
            "ClientMessageId": client_message_id,
            "Content": content,
        })

    def mark_direct_messages_read(self, peer_user_id, through_message_id):
        return self.invoke("MarkDirectMessagesRead", {
            "PeerUserId": int(peer_user_id),
            "ThroughMessageId": int(through_message_id),
        })
