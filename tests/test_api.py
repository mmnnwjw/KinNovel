import unittest

from kinnovel.api import ApiClient, _normalize_data, _normalize_list, dict_items, int_items


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

        def fake_invoke(method, params):
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

        def fake_invoke(method, params):
            captured["params"] = params
            return []

        client.invoke = fake_invoke
        client.get_book_list_by_ids([1], "Comic")
        self.assertEqual(captured["params"]["Type"], "Comic")


if __name__ == "__main__":
    unittest.main()
