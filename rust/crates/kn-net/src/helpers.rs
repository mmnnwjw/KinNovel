//! Port of the free helper functions at the top of `api.py`: normalizing
//! list/envelope shapes the hub can return, and filtering out comics from
//! novel-only lists (comics have their own endpoints and reader).

use serde_json::{Map, Value};

/// Only keep dict/object elements -- the server can return `null` for
/// invalidated ids (`api.py`'s `dict_items`).
pub fn dict_items(value: &Value) -> Vec<Value> {
    match value.as_array() {
        Some(arr) => arr.iter().filter(|v| v.is_object()).cloned().collect(),
        None => Vec::new(),
    }
}

pub fn int_items(value: &Value) -> Vec<i64> {
    match value.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|v| match v {
                Value::Number(n) => n.as_i64(),
                Value::String(s) => s.parse().ok(),
                _ => None,
            })
            .collect(),
        None => Vec::new(),
    }
}

/// `{Data: [...]}` -> same object with `Data` filtered to objects only.
pub fn normalize_data(envelope: Value, key: &str) -> Value {
    match envelope {
        Value::Object(mut obj) => {
            replace_in_place(&mut obj, key, dict_items_owned);
            Value::Object(obj)
        }
        other => other,
    }
}

/// 同 [`dict_items`], 按值取走, 不复制元素。
fn dict_items_owned(value: Value) -> Vec<Value> {
    match value {
        Value::Array(arr) => arr.into_iter().filter(Value::is_object).collect(),
        _ => Vec::new(),
    }
}

/// `obj[key] = Array(f(旧值))`, 原地替换 (不复制旧值; 开了 preserve_order 时键的位置也不变)。
fn replace_in_place(obj: &mut Map<String, Value>, key: &str, f: fn(Value) -> Vec<Value>) {
    match obj.get_mut(key) {
        Some(slot) => {
            let data = std::mem::take(slot);
            *slot = Value::Array(f(data));
        }
        None => {
            obj.insert(key.to_string(), Value::Array(f(Value::Null)));
        }
    }
}

/// Accepts either a bare array or `{Data: [...]}` and returns a normalized
/// value in the same shape it was given (`api.py`'s `_normalize_list`).
pub fn normalize_list(value: Value, key: &str) -> Value {
    if value.is_object() {
        normalize_data(value, key)
    } else {
        Value::Array(dict_items_owned(value))
    }
}

/// List endpoints use `Type=Novel/Comic`; the shelf uses lowercase
/// `type=NOVEL/COMIC` (`api.py`'s `is_comic`).
pub fn is_comic(item: &Value) -> bool {
    let Some(obj) = item.as_object() else { return false };
    let value = obj.get("Type").or_else(|| obj.get("type"));
    match value.and_then(Value::as_str) {
        Some(s) => s.trim().eq_ignore_ascii_case("comic"),
        None => false,
    }
}

pub fn novel_items(value: &Value) -> Vec<Value> {
    dict_items(value).into_iter().filter(|item| !is_comic(item)).collect()
}

/// Normalize and drop comics, accepting either a bare array or `{Data: [...]}`
/// (`api.py`'s `_novel_data`).
pub fn novel_data(value: Value, key: &str) -> Value {
    match value {
        Value::Object(mut obj) => {
            replace_in_place(&mut obj, key, novel_items_owned);
            Value::Object(obj)
        }
        other => Value::Array(novel_items_owned(other)),
    }
}

fn novel_items_owned(value: Value) -> Vec<Value> {
    dict_items_owned(value).into_iter().filter(|item| !is_comic(item)).collect()
}

pub fn object(pairs: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    for (k, v) in pairs {
        map.insert(k.to_string(), v);
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dict_items_filters_non_objects() {
        let value = json!([{"Id": 1}, null, "x", {"Id": 2}]);
        assert_eq!(dict_items(&value).len(), 2);
    }

    #[test]
    fn int_items_filters_invalid() {
        let value = json!([1, "2", "x", null, 3.0]);
        assert_eq!(int_items(&value), vec![1, 2]);
    }

    #[test]
    fn is_comic_handles_both_field_cases() {
        assert!(is_comic(&json!({"Type": "Comic"})));
        assert!(is_comic(&json!({"type": "COMIC"})));
        assert!(!is_comic(&json!({"Type": "Novel"})));
        assert!(!is_comic(&json!({})));
    }

    #[test]
    fn novel_items_drops_comics() {
        let value = json!([{"Type": "Novel", "Id": 1}, {"Type": "Comic", "Id": 2}]);
        let result = novel_items(&value);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["Id"], 1);
    }

    #[test]
    fn normalize_list_handles_bare_array_and_envelope() {
        let bare = json!([{"Id": 1}, null]);
        assert_eq!(normalize_list(bare, "Data"), json!([{"Id": 1}]));

        let envelope = json!({"Data": [{"Id": 1}, "junk"], "TotalPages": 2});
        let normalized = normalize_list(envelope, "Data");
        assert_eq!(normalized["Data"], json!([{"Id": 1}]));
        assert_eq!(normalized["TotalPages"], 2);
    }

    #[test]
    fn normalize_keeps_key_order_and_missing_key() {
        let envelope = json!({"A": 1, "Data": [{"Id": 1}, null], "Z": 2});
        let keys: Vec<String> = normalize_data(envelope, "Data").as_object().unwrap().keys().cloned().collect();
        let expected: Vec<String> = json!({"A": 1, "Data": [], "Z": 2}).as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, expected);
        assert_eq!(normalize_data(json!({"X": 1}), "Data")["Data"], json!([]));
        assert_eq!(novel_data(json!({"Data": "junk"}), "Data")["Data"], json!([]));
        assert_eq!(novel_data(json!([{"Type": "Comic"}, {"Id": 3}, 1]), "Data"), json!([{"Id": 3}]));
    }

    #[test]
    fn novel_data_filters_comics_from_envelope() {
        let envelope = json!({"Data": [{"Type": "Novel"}, {"Type": "Comic"}]});
        let result = novel_data(envelope, "Data");
        assert_eq!(result["Data"].as_array().unwrap().len(), 1);
    }
}
