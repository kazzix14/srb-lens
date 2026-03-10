use serde::{Deserialize, Serialize};
use serde_json::Value;

/// メソッドのソースコード上の位置情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodLoc {
    pub file: String,
    pub name: String,
    pub line: usize,
    pub is_class_method: bool,
    #[serde(default)]
    pub sig_return_type: Option<String>,
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

    // Begin ブロックは stmts を順に走査して sig → DefMethod/DefS ペアを検出
    if type_str == "Begin" {
        if let Some(Value::Array(stmts)) = obj.get("stmts") {
            walk_stmts(stmts, locs);
            return;
        }
    }

    match type_str {
        "DefMethod" => {
            if let Some(loc) = extract_method_loc(obj, false, None) {
                locs.push(loc);
            }
        }
        "DefS" => {
            if let Some(loc) = extract_method_loc(obj, true, None) {
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

/// Begin.stmts を順に走査し、sig ブロック → DefMethod/DefS ペアを検出する
fn walk_stmts(stmts: &[Value], locs: &mut Vec<MethodLoc>) {
    let mut pending_sig_return: Option<String> = None;

    for stmt in stmts {
        let Some(obj) = stmt.as_object() else {
            pending_sig_return = None;
            continue;
        };
        let type_str = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match type_str {
            "Block" => {
                // sig { ... } は Block(send: Send(method: "sig"), body: ...)
                if is_sig_block(obj) {
                    pending_sig_return = extract_sig_return_type(obj);
                } else {
                    pending_sig_return = None;
                    walk_value(stmt, locs);
                }
            }
            "DefMethod" => {
                if let Some(loc) = extract_method_loc(obj, false, pending_sig_return.take()) {
                    locs.push(loc);
                }
                // 再帰: メソッド body 内のネストされたメソッド定義
                walk_children(obj, locs);
            }
            "DefS" => {
                if let Some(loc) = extract_method_loc(obj, true, pending_sig_return.take()) {
                    locs.push(loc);
                }
                walk_children(obj, locs);
            }
            _ => {
                pending_sig_return = None;
                walk_value(stmt, locs);
            }
        }
    }
}

/// オブジェクトの子ノード(body等)を再帰走査する（DefMethod/DefS 自体は処理済み前提）
fn walk_children(obj: &serde_json::Map<String, Value>, locs: &mut Vec<MethodLoc>) {
    for (key, v) in obj {
        if key == "declLoc" || key == "name" || key == "args" {
            continue;
        }
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

/// Block ノードが sig ブロックかどうか判定
fn is_sig_block(obj: &serde_json::Map<String, Value>) -> bool {
    let send = match obj.get("send") {
        Some(v) => v,
        None => return false,
    };
    let send_obj = match send.as_object() {
        Some(o) => o,
        None => return false,
    };
    send_obj
        .get("method")
        .and_then(|v| v.as_str())
        .map_or(false, |m| m == "sig")
}

/// sig ブロックの body から returns(X) の型を抽出
fn extract_sig_return_type(block_obj: &serde_json::Map<String, Value>) -> Option<String> {
    let body = block_obj.get("body")?;
    find_returns_in_send(body)
}

/// Send チェーンを再帰探索して method == "returns" を持つノードを見つけ、
/// その args[0] を型文字列に変換する。
/// `params(...).returns(X)` や `override.returns(X)` のようなチェーンに対応。
fn find_returns_in_send(value: &Value) -> Option<String> {
    let obj = value.as_object()?;
    let type_str = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if type_str != "Send" {
        return None;
    }

    let method = obj.get("method").and_then(|v| v.as_str()).unwrap_or("");
    if method == "returns" {
        let args = obj.get("args")?.as_array()?;
        let first_arg = args.first()?;
        let type_str = ast_type_to_string(first_arg)?;
        // void は sig_return_type を None にする（CFG に任せる）
        if type_str == "void" {
            return None;
        }
        return Some(type_str);
    }

    // チェーン: receiver 側にも returns があるかもしれない
    if let Some(receiver) = obj.get("receiver") {
        if let Some(result) = find_returns_in_send(receiver) {
            return Some(result);
        }
    }
    None
}

/// AST の型ノードを文字列に変換する
///
/// | AST                                                           | 出力                    |
/// |---------------------------------------------------------------|-------------------------|
/// | Const(scope: null, name: "String")                            | "String"                |
/// | Const(scope: Const("Booth"), name: "PrivateRelation")         | "Booth::PrivateRelation"|
/// | Send(receiver: Const("T"), method: "nilable", args: [X])      | "T.nilable(X)"          |
/// | Send(method: "[]", receiver: Const(T::Array), args: [X])      | "T::Array[X]"           |
/// | Send(receiver: Const("T"), method: "any", args: [X,Y])        | "T.any(X, Y)"           |
fn ast_type_to_string(value: &Value) -> Option<String> {
    let obj = value.as_object()?;
    let type_str = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match type_str {
        "Const" | "UnresolvedConstantLit" => {
            let name = obj.get("name").and_then(|v| v.as_str())?;
            let scope = obj.get("scope");
            match scope {
                Some(Value::Null) | None => Some(name.to_string()),
                Some(scope_val) => {
                    let scope_str = ast_type_to_string(scope_val)?;
                    Some(format!("{scope_str}::{name}"))
                }
            }
        }
        "Send" => {
            let method = obj.get("method").and_then(|v| v.as_str()).unwrap_or("");
            let args = obj
                .get("args")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let arg_strs: Vec<String> = args.iter().filter_map(ast_type_to_string).collect();

            if method == "[]" {
                // T::Array[X] or T::Hash[K, V]
                let receiver = obj.get("receiver")?;
                let recv_str = ast_type_to_string(receiver)?;
                Some(format!("{recv_str}[{}]", arg_strs.join(", ")))
            } else if method == "void" {
                Some("void".to_string())
            } else {
                // T.nilable(X), T.any(X, Y), T.class_of(X), etc.
                let receiver = obj.get("receiver");
                match receiver {
                    Some(Value::Null) | None => Some(method.to_string()),
                    Some(recv_val) => {
                        let recv_str = ast_type_to_string(recv_val)?;
                        if arg_strs.is_empty() {
                            Some(format!("{recv_str}.{method}"))
                        } else {
                            Some(format!("{recv_str}.{method}({})", arg_strs.join(", ")))
                        }
                    }
                }
            }
        }
        _ => None,
    }
}

fn extract_method_loc(
    obj: &serde_json::Map<String, Value>,
    is_class_method: bool,
    sig_return_type: Option<String>,
) -> Option<MethodLoc> {
    let name = obj.get("name")?.as_str()?;
    let decl_loc = obj.get("declLoc")?.as_str()?;
    let (file, line) = parse_decl_loc(decl_loc)?;

    Some(MethodLoc {
        file,
        name: name.to_string(),
        line,
        is_class_method,
        sig_return_type,
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

    #[test]
    fn test_sig_returns_simple_type() {
        // sig { returns(Booth::PrivateRelation) } + def booths
        let input = r#"{
            "type": "Class",
            "declLoc": "app/models/foo.rb:1:1-20:4",
            "name": { "type": "Const", "scope": null, "name": "Foo" },
            "superclass": null,
            "body": {
                "type": "Begin",
                "stmts": [
                    {
                        "type": "Block",
                        "send": { "type": "Send", "receiver": null, "method": "sig", "args": [] },
                        "args": null,
                        "body": {
                            "type": "Send",
                            "receiver": null,
                            "method": "returns",
                            "args": [
                                {
                                    "type": "Const",
                                    "scope": { "type": "Const", "scope": null, "name": "Booth" },
                                    "name": "PrivateRelation"
                                }
                            ]
                        }
                    },
                    {
                        "type": "DefMethod",
                        "declLoc": "app/models/foo.rb:5:3-7:6",
                        "name": "booths",
                        "args": null,
                        "body": null
                    }
                ]
            }
        }"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].name, "booths");
        assert_eq!(
            locs[0].sig_return_type.as_deref(),
            Some("Booth::PrivateRelation")
        );
    }

    #[test]
    fn test_sig_params_returns_chain() {
        // sig { params(id: Integer).returns(String) } + def find
        let input = r#"{
            "type": "Class",
            "declLoc": "a.rb:1:1-20:4",
            "name": { "type": "Const", "scope": null, "name": "Foo" },
            "superclass": null,
            "body": {
                "type": "Begin",
                "stmts": [
                    {
                        "type": "Block",
                        "send": { "type": "Send", "receiver": null, "method": "sig", "args": [] },
                        "args": null,
                        "body": {
                            "type": "Send",
                            "receiver": {
                                "type": "Send",
                                "receiver": null,
                                "method": "params",
                                "args": []
                            },
                            "method": "returns",
                            "args": [
                                { "type": "Const", "scope": null, "name": "String" }
                            ]
                        }
                    },
                    {
                        "type": "DefMethod",
                        "declLoc": "a.rb:5:3-7:6",
                        "name": "find",
                        "args": null,
                        "body": null
                    }
                ]
            }
        }"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].name, "find");
        assert_eq!(locs[0].sig_return_type.as_deref(), Some("String"));
    }

    #[test]
    fn test_sig_void_returns_none() {
        // sig { void } → sig_return_type should be None
        let input = r#"{
            "type": "Class",
            "declLoc": "a.rb:1:1-20:4",
            "name": { "type": "Const", "scope": null, "name": "Foo" },
            "superclass": null,
            "body": {
                "type": "Begin",
                "stmts": [
                    {
                        "type": "Block",
                        "send": { "type": "Send", "receiver": null, "method": "sig", "args": [] },
                        "args": null,
                        "body": {
                            "type": "Send",
                            "receiver": null,
                            "method": "void",
                            "args": []
                        }
                    },
                    {
                        "type": "DefMethod",
                        "declLoc": "a.rb:5:3-7:6",
                        "name": "reset!",
                        "args": null,
                        "body": null
                    }
                ]
            }
        }"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].name, "reset!");
        assert!(locs[0].sig_return_type.is_none());
    }

    #[test]
    fn test_sig_nilable_type() {
        // sig { returns(T.nilable(String)) }
        let input = r#"{
            "type": "Class",
            "declLoc": "a.rb:1:1-20:4",
            "name": { "type": "Const", "scope": null, "name": "Foo" },
            "superclass": null,
            "body": {
                "type": "Begin",
                "stmts": [
                    {
                        "type": "Block",
                        "send": { "type": "Send", "receiver": null, "method": "sig", "args": [] },
                        "args": null,
                        "body": {
                            "type": "Send",
                            "receiver": null,
                            "method": "returns",
                            "args": [
                                {
                                    "type": "Send",
                                    "receiver": { "type": "Const", "scope": null, "name": "T" },
                                    "method": "nilable",
                                    "args": [
                                        { "type": "Const", "scope": null, "name": "String" }
                                    ]
                                }
                            ]
                        }
                    },
                    {
                        "type": "DefMethod",
                        "declLoc": "a.rb:5:3-7:6",
                        "name": "maybe_name",
                        "args": null,
                        "body": null
                    }
                ]
            }
        }"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 1);
        assert_eq!(
            locs[0].sig_return_type.as_deref(),
            Some("T.nilable(String)")
        );
    }

    #[test]
    fn test_sig_array_type() {
        // sig { returns(T::Array[String]) }
        let input = r#"{
            "type": "Class",
            "declLoc": "a.rb:1:1-20:4",
            "name": { "type": "Const", "scope": null, "name": "Foo" },
            "superclass": null,
            "body": {
                "type": "Begin",
                "stmts": [
                    {
                        "type": "Block",
                        "send": { "type": "Send", "receiver": null, "method": "sig", "args": [] },
                        "args": null,
                        "body": {
                            "type": "Send",
                            "receiver": null,
                            "method": "returns",
                            "args": [
                                {
                                    "type": "Send",
                                    "receiver": {
                                        "type": "Const",
                                        "scope": { "type": "Const", "scope": null, "name": "T" },
                                        "name": "Array"
                                    },
                                    "method": "[]",
                                    "args": [
                                        { "type": "Const", "scope": null, "name": "String" }
                                    ]
                                }
                            ]
                        }
                    },
                    {
                        "type": "DefMethod",
                        "declLoc": "a.rb:5:3-7:6",
                        "name": "names",
                        "args": null,
                        "body": null
                    }
                ]
            }
        }"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 1);
        assert_eq!(
            locs[0].sig_return_type.as_deref(),
            Some("T::Array[String]")
        );
    }

    #[test]
    fn test_no_sig_returns_none() {
        // Method without preceding sig
        let input = r#"{
            "type": "Class",
            "declLoc": "a.rb:1:1-10:4",
            "name": { "type": "Const", "scope": null, "name": "Foo" },
            "superclass": null,
            "body": {
                "type": "Begin",
                "stmts": [
                    {
                        "type": "DefMethod",
                        "declLoc": "a.rb:3:3-5:6",
                        "name": "bar",
                        "args": null,
                        "body": null
                    }
                ]
            }
        }"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 1);
        assert!(locs[0].sig_return_type.is_none());
    }

    #[test]
    fn test_sig_not_applied_to_wrong_method() {
        // sig + method1, method2 (no sig) — method2 should NOT get the sig
        let input = r#"{
            "type": "Class",
            "declLoc": "a.rb:1:1-20:4",
            "name": { "type": "Const", "scope": null, "name": "Foo" },
            "superclass": null,
            "body": {
                "type": "Begin",
                "stmts": [
                    {
                        "type": "Block",
                        "send": { "type": "Send", "receiver": null, "method": "sig", "args": [] },
                        "args": null,
                        "body": {
                            "type": "Send",
                            "receiver": null,
                            "method": "returns",
                            "args": [
                                { "type": "Const", "scope": null, "name": "String" }
                            ]
                        }
                    },
                    {
                        "type": "DefMethod",
                        "declLoc": "a.rb:5:3-7:6",
                        "name": "with_sig",
                        "args": null,
                        "body": null
                    },
                    {
                        "type": "DefMethod",
                        "declLoc": "a.rb:9:3-11:6",
                        "name": "without_sig",
                        "args": null,
                        "body": null
                    }
                ]
            }
        }"#;

        let locs = parse(input).unwrap();
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0].name, "with_sig");
        assert_eq!(locs[0].sig_return_type.as_deref(), Some("String"));
        assert_eq!(locs[1].name, "without_sig");
        assert!(locs[1].sig_return_type.is_none());
    }
}
