use serde::{Deserialize, Serialize};
use serde_json::Value;

/// メソッドのソースコード上の位置情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodLoc {
    pub file: String,
    pub name: String,
    pub line: usize,
    pub is_class_method: bool,
}

/// parse-tree-json-with-locs の出力から DefMethod/DefS のメソッド位置を抽出する。
///
/// 出力は改行区切りの JSON オブジェクト（1トップレベル定義 = 1 JSON）。
/// 各 JSON を再帰的に走査して DefMethod/DefS を収集する。
pub fn parse(input: &str) -> Result<Vec<MethodLoc>, ParseTreeError> {
    let mut locs = Vec::new();
    let stream = serde_json::Deserializer::from_str(input).into_iter::<Value>();

    for item in stream {
        let value = item.map_err(|e| ParseTreeError::Json(e.to_string()))?;
        walk_value(&value, &mut locs);
    }

    Ok(locs)
}

fn walk_value(value: &Value, locs: &mut Vec<MethodLoc>) {
    let Some(obj) = value.as_object() else {
        return;
    };

    let type_str = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match type_str {
        "DefMethod" => {
            if let Some(loc) = extract_method_loc(obj, false) {
                locs.push(loc);
            }
        }
        "DefS" => {
            if let Some(loc) = extract_method_loc(obj, true) {
                locs.push(loc);
            }
        }
        _ => {}
    }

    // 再帰: 全フィールドの子ノードを走査
    for (_, v) in obj {
        match v {
            Value::Object(_) => walk_value(v, locs),
            Value::Array(arr) => {
                for item in arr {
                    walk_value(item, locs);
                }
            }
            _ => {}
        }
    }
}

fn extract_method_loc(
    obj: &serde_json::Map<String, Value>,
    is_class_method: bool,
) -> Option<MethodLoc> {
    let name = obj.get("name")?.as_str()?;
    let decl_loc = obj.get("declLoc")?.as_str()?;
    let (file, line) = parse_decl_loc(decl_loc)?;

    Some(MethodLoc {
        file,
        name: name.to_string(),
        line,
        is_class_method,
    })
}

/// declLoc フォーマット: "path/to/file.rb:START_LINE:START_COL-END_LINE:END_COL"
/// 例: "app/models/campaign.rb:42:5-44:8" → ("app/models/campaign.rb", 42)
fn parse_decl_loc(decl_loc: &str) -> Option<(String, usize)> {
    // 末尾から "-END_LINE:END_COL" を除去
    let before_dash = decl_loc.rsplit_once('-')?.0;
    // 末尾から ":START_COL" を除去
    let before_col = before_dash.rsplit_once(':')?.0;
    // 末尾から ":START_LINE" を取得
    let (file, line_str) = before_col.rsplit_once(':')?;
    let line = line_str.parse().ok()?;
    Some((file.to_string(), line))
}

#[derive(Debug, thiserror::Error)]
pub enum ParseTreeError {
    #[error("JSON parse error: {0}")]
    Json(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_decl_loc() {
        let (file, line) = parse_decl_loc("app/models/campaign.rb:42:5-44:8").unwrap();
        assert_eq!(file, "app/models/campaign.rb");
        assert_eq!(line, 42);
    }

    #[test]
    fn test_parse_def_method() {
        let input = r#"{
            "type": "Class",
            "declLoc": "app/models/foo.rb:1:1-10:4",
            "name": { "type": "Const", "scope": null, "name": "Foo" },
            "superclass": null,
            "body": {
                "type": "DefMethod",
                "declLoc": "app/models/foo.rb:3:3-5:6",
                "name": "bar",
                "args": null,
                "body": null
            }
        }"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].file, "app/models/foo.rb");
        assert_eq!(locs[0].name, "bar");
        assert_eq!(locs[0].line, 3);
        assert!(!locs[0].is_class_method);
    }

    #[test]
    fn test_parse_def_s() {
        let input = r#"{
            "type": "Class",
            "declLoc": "app/models/foo.rb:1:1-10:4",
            "name": { "type": "Const", "scope": null, "name": "Foo" },
            "superclass": null,
            "body": {
                "type": "DefS",
                "declLoc": "app/models/foo.rb:7:3-9:6",
                "singleton": { "type": "Self" },
                "name": "create",
                "args": null,
                "body": null
            }
        }"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].name, "create");
        assert_eq!(locs[0].line, 7);
        assert!(locs[0].is_class_method);
    }

    #[test]
    fn test_parse_multiple_methods_in_begin() {
        let input = r#"{
            "type": "Class",
            "declLoc": "app/models/foo.rb:1:1-20:4",
            "name": { "type": "Const", "scope": null, "name": "Foo" },
            "superclass": null,
            "body": {
                "type": "Begin",
                "stmts": [
                    {
                        "type": "DefMethod",
                        "declLoc": "app/models/foo.rb:3:3-5:6",
                        "name": "alpha",
                        "args": null,
                        "body": null
                    },
                    {
                        "type": "DefMethod",
                        "declLoc": "app/models/foo.rb:7:3-9:6",
                        "name": "beta",
                        "args": null,
                        "body": null
                    }
                ]
            }
        }"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0].name, "alpha");
        assert_eq!(locs[1].name, "beta");
    }

    #[test]
    fn test_parse_stream_multiple_top_level() {
        let input = r#"{"type":"DefMethod","declLoc":"a.rb:1:1-2:4","name":"x","args":null,"body":null}
{"type":"DefMethod","declLoc":"b.rb:5:1-6:4","name":"y","args":null,"body":null}"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0].file, "a.rb");
        assert_eq!(locs[1].file, "b.rb");
    }
}
