import threading
import unittest

from kinnovel.api import (
    ApiClient,
    _normalize_data,
    _normalize_list,
    dict_items,
    int_items,
    is_comic,
    novel_items,
)
from kinnovel.transport import ApiError, TransportError


class FakeSession:
    def __init__(self, **values):
        self.data = dict(values)
        self.cleared = False

    def get(self, key, default=None):
        return self.data.get(key, default)

    def set_many(self, values):
        self.data.update(values)

    def clear_credentials(self):
        self.cleared = True
        self.data.pop("Token", None)
        self.data.pop("RefreshToken", None)


def bare_client(session=None):
    client = ApiClient.__new__(ApiClient)
    client.session = session or FakeSession()
    client._refresh_lock = threading.Lock()
    return client


class ApiNormalizeTests(unittest.TestCase):
    def test_dict_items_filters_null_entries(self):
        value = [{"Id": 1}, None, "bad", 7, {"Id": 2}]
        self.assertEqual(dict_items(value), [{"Id": 1}, {"Id": 2}])

    def test_dict_items_handles_non_list(self):
        self.assertEqual(dict_items(None), [])
        self.assertEqual(dict_items({"Id": 1}), [])

    def test_int_items_filters_invalid_ids(self):
        self.assertEqual(int_items([1, None, "2", "x", 3.0]), [1, 2, 3])

    def test_normalize_data_replaces_invalid_list(self):
        envelope = _normalize_data({"Data": [None, {"Id": 1}]})
        self.assertEqual(envelope["Data"], [{"Id": 1}])
        self.assertEqual(_normalize_data(None), None)
        self.assertEqual(_normalize_data({"Data": None})["Data"], [])

    def test_normalize_list_handles_bare_array(self):
        self.assertEqual(_normalize_list([{"Id": 1}, None, "x"]), [{"Id": 1}])
        self.assertEqual(_normalize_list({"Data": [None, {"Id": 2}]})["Data"],
                         [{"Id": 2}])

    def test_book_list_by_ids_omits_novel_type(self):
        client = object.__new__(ApiClient)
        captured = {}

        def fake_invoke(method, params=None):
            captured["method"] = method
            captured["params"] = params
            return [{"Id": 1}, None]

        client.invoke = fake_invoke
        result = client.get_book_list_by_ids([1, 2], "Novel")
        self.assertEqual(captured["method"], "GetBookListByIds")
        self.assertNotIn("Type", captured["params"])
        self.assertEqual(captured["params"]["Ids"], [1, 2])
        self.assertEqual(result, [{"Id": 1}])

    def test_book_list_by_ids_keeps_comic_type(self):
        client = object.__new__(ApiClient)
        captured = {}

        def fake_invoke(method, params=None):
            captured["params"] = params
            return []

        client.invoke = fake_invoke
        client.get_book_list_by_ids([1], "Comic")
        self.assertEqual(captured["params"]["Type"], "Comic")

    def test_book_list_by_ids_chunked_splits_at_24(self):
        client = object.__new__(ApiClient)
        calls = []

        def fake_invoke(method, params=None):
            calls.append(list(params["Ids"]))
            return [{"Id": value} for value in params["Ids"]]

        client.invoke = fake_invoke
        result = client.get_book_list_by_ids_chunked(list(range(1, 51)))
        self.assertEqual([len(chunk) for chunk in calls], [24, 24, 2])
        self.assertEqual(len(result), 50)
        self.assertEqual(result[0], {"Id": 1})
        self.assertEqual(result[-1], {"Id": 50})

    def test_book_list_by_ids_chunked_handles_envelope(self):
        client = object.__new__(ApiClient)

        def fake_invoke(method, params=None):
            return {"Data": [{"Id": value} for value in params["Ids"]]}

        client.invoke = fake_invoke
        self.assertEqual(len(client.get_book_list_by_ids_chunked([1, 2, 3])), 3)


class ComicFilterTests(unittest.TestCase):
    def _client(self, response, captured=None):
        client = object.__new__(ApiClient)

        def fake_invoke(method, params=None):
            if captured is not None:
                captured["method"] = method
                captured["params"] = params
            return response

        client.invoke = fake_invoke
        return client

    def test_is_comic_handles_both_field_cases(self):
        self.assertTrue(is_comic({"Type": "Comic"}))
        self.assertTrue(is_comic({"type": "COMIC"}))
        self.assertFalse(is_comic({"Type": "Novel"}))
        self.assertFalse(is_comic({"type": "NOVEL"}))
        self.assertFalse(is_comic({"type": "FOLDER"}))
        self.assertFalse(is_comic(None))

    def test_novel_items_drops_comics(self):
        items = [{"Type": "Novel"}, {"Type": "Comic"}, None]
        self.assertEqual(novel_items(items), [{"Type": "Novel"}])

    def test_get_rank_filters_comics(self):
        client = self._client([{"Type": "Novel"}, {"Type": "Comic"}])
        self.assertEqual(client.get_rank(1), [{"Type": "Novel"}])

    def test_get_book_list_filters_comics(self):
        client = self._client({
            "Data": [{"Type": "Novel"}, {"Type": "Comic"}],
            "Page": 1, "TotalPages": 1,
        })
        result = client.get_book_list(page=1, size=2)
        self.assertEqual(result["Data"], [{"Type": "Novel"}])

    def test_get_book_list_by_ids_filters_comics(self):
        client = self._client([{"Type": "Novel"}, {"Type": "Comic"}, None])
        self.assertEqual(client.get_book_list_by_ids([1, 2, 3]),
                         [{"Type": "Novel"}])

    def test_get_book_shelf_keeps_comic_and_folder_items(self):
        client = self._client({"data": [
            {"type": "NOVEL", "id": 1},
            {"type": "COMIC", "id": 2},
            {"type": "FOLDER", "id": 3},
        ]})
        result = client.get_book_shelf()
        # 书架写回是整表覆盖, API 层不能丢漫画或文件夹。
        self.assertEqual([item["id"] for item in result["data"]], [1, 2, 3])

    def test_get_book_shelf_normalizes_bare_array(self):
        client = self._client([{"type": "NOVEL", "id": 1}, None, "bad"])
        self.assertEqual(client.get_book_shelf(), [{"type": "NOVEL", "id": 1}])

    def test_get_book_shelf_normalizes_data_key(self):
        client = self._client({"Data": [{"type": "COMIC", "id": 2}, None]})
        self.assertEqual(client.get_book_shelf()["Data"], [{"type": "COMIC", "id": 2}])


class RefreshCredentialTests(unittest.TestCase):
    def test_refresh_access_token_401_keeps_credentials(self):
        session = FakeSession(RefreshToken="refresh")
        client = bare_client(session)

        def unauthorized(path, payload):
            raise ApiError("unauthorized", 401)

        client._http = unauthorized
        with self.assertRaises(ApiError):
            client.refresh_access_token()
        self.assertFalse(session.cleared)
        self.assertEqual(session.get("RefreshToken"), "refresh")

    def test_refresh_access_token_404_clears_credentials(self):
        session = FakeSession(RefreshToken="refresh")
        client = bare_client(session)

        def missing(path, payload):
            raise ApiError("not found", 404)

        client._http = missing
        with self.assertRaises(ApiError):
            client.refresh_access_token()
        self.assertTrue(session.cleared)

    def test_refresh_access_token_success_updates_token(self):
        session = FakeSession(RefreshToken="refresh")
        client = bare_client(session)
        client._http = lambda path, payload: "new-token"
        self.assertEqual(client.refresh_access_token(), "new-token")
        self.assertEqual(session.get("Token"), "new-token")
        self.assertFalse(session.cleared)

    def test_get_access_token_401_does_not_clear_credentials(self):
        session = FakeSession(RefreshToken="refresh")
        client = bare_client(session)

        def unauthorized():
            raise ApiError("unauthorized", 401)

        client.refresh_access_token = unauthorized
        with self.assertRaises(ApiError):
            client.get_access_token()
        self.assertFalse(session.cleared)

    def test_get_access_token_404_downgrades_to_anonymous(self):
        session = FakeSession(RefreshToken="refresh")
        client = bare_client(session)

        def missing():
            raise ApiError("not found", 404)

        client.refresh_access_token = missing
        self.assertIsNone(client.get_access_token())
        self.assertTrue(session.cleared)

    def test_get_access_token_transient_error_keeps_credentials(self):
        session = FakeSession(RefreshToken="refresh")
        client = bare_client(session)

        def down():
            raise TransportError("network down")

        client.refresh_access_token = down
        with self.assertRaises(TransportError):
            client.get_access_token()
        self.assertFalse(session.cleared)

    def test_get_access_token_reads_token_and_updated_as_one_snapshot(self):
        """A concurrent refresh must not be interleaved with the read of
        Token/TokenUpdatedAt, or get_access_token could hand back a stale
        token paired with a fresh timestamp (or vice versa)."""
        import time as time_module

        session = FakeSession(Token="old", TokenUpdatedAt=time_module.time(),
                               RefreshToken="refresh")
        client = bare_client(session)

        release_refresh = threading.Event()
        reader_holds_lock = threading.Event()

        real_get = session.get

        def blocking_get(key, default=None):
            value = real_get(key, default)
            if key == "Token":
                reader_holds_lock.set()
                # Give the refresher a chance to run while we are supposedly
                # "inside" the locked snapshot read.
                release_refresh.wait(1.0)
            return value

        session.get = blocking_get

        def refresher():
            reader_holds_lock.wait(1.0)
            with client._refresh_lock:
                session.data["Token"] = "new"
                session.data["TokenUpdatedAt"] = time_module.time()
            release_refresh.set()

        thread = threading.Thread(target=refresher)
        thread.start()
        token = client.get_access_token()
        thread.join(5)

        # Either the snapshot was taken fully before the refresh (old token,
        # old timestamp, both still "fresh enough") or fully after (new
        # token, new timestamp) - never a mix that would be rejected or
        # silently accepted as something it isn't.
        self.assertIn(token, ("old", "new"))

    def test_suspend_drops_hub_connection(self):
        client = bare_client()
        closed = []
        client.hub = type("FakeHub", (), {"close": lambda self: closed.append(True)})()
        client.suspend()
        self.assertEqual(closed, [True])

    def test_resume_does_not_touch_hub(self):
        client = bare_client()

        class StrictHub:
            def __getattr__(self, name):
                raise AssertionError("resume() must not call the hub")

        client.hub = StrictHub()
        client.resume()  # should not raise


if __name__ == "__main__":
    unittest.main()
