use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug)]
pub struct SymbolTree {
    pub root: RawSymbol,
}

#[derive(Debug, Deserialize)]
pub struct RawSymbol {
    pub id: u64,
    pub name: SymbolName,
    pub kind: String,
    #[serde(rename = "superClass")]
    pub super_class: Option<u64>,
    pub mixins: Option<Vec<u64>>,
    #[serde(rename = "isModule")]
    pub is_module: Option<bool>,
    pub arguments: Option<Vec<RawArgument>>,
    pub children: Option<Vec<RawSymbol>>,
}

#[derive(Debug, Deserialize)]
pub struct SymbolName {
    pub kind: String,
    pub name: String,
    pub unique: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawArgument {
    pub name: SymbolName,
    #[serde(rename = "isBlock")]
    pub is_block: Option<bool>,
}

impl SymbolTree {
    pub fn build_id_map(&self) -> HashMap<u64, String> {
        let mut map = HashMap::new();
        build_id_map_recursive(&self.root, "", &mut map);
        map
    }
}

fn build_id_map_recursive(symbol: &RawSymbol, parent_fqn: &str, map: &mut HashMap<u64, String>) {
    let fqn = if parent_fqn.is_empty() || symbol.name.name == "<root>" {
        symbol.name.name.clone()
    } else {
        format!("{}::{}", parent_fqn, symbol.name.name)
    };

    if symbol.name.name != "<root>" {
        map.insert(symbol.id, fqn.clone());
    }

    if let Some(children) = &symbol.children {
        let current_fqn = if symbol.name.name == "<root>" {
            ""
        } else {
            &fqn
        };
        for child in children {
            build_id_map_recursive(child, current_fqn, map);
        }
    }
}

pub fn parse(json: &str) -> Result<SymbolTree, SymbolTableParseError> {
    let root: RawSymbol = serde_json::from_str(json)?;
    Ok(SymbolTree { root })
}

#[derive(Debug, thiserror::Error)]
pub enum SymbolTableParseError {
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_id_map() {
        let json = r#"{
            "id": 24,
            "name": { "kind": "CONSTANT", "name": "<root>" },
            "kind": "CLASS_OR_MODULE",
            "superClass": 48,
            "children": [
                {
                    "id": 32336,
                    "name": { "kind": "CONSTANT", "name": "Booth" },
                    "kind": "CLASS_OR_MODULE",
                    "superClass": 32288,
                    "mixins": [42464, 42432],
                    "children": [
                        {
                            "id": 308897,
                            "name": { "kind": "UTF8", "name": "archive!" },
                            "kind": "METHOD",
                            "arguments": [
                                { "name": { "kind": "UTF8", "name": "<blk>" }, "isBlock": true }
                            ]
                        }
                    ]
                },
                {
                    "id": 32288,
                    "name": { "kind": "CONSTANT", "name": "ApplicationRecord" },
                    "kind": "CLASS_OR_MODULE",
                    "superClass": 48
                }
            ]
        }"#;

        let tree = parse(json).unwrap();
        assert_eq!(tree.root.name.name, "<root>");

        let id_map = tree.build_id_map();
        assert_eq!(id_map.get(&32336), Some(&"Booth".to_string()));
        assert_eq!(id_map.get(&32288), Some(&"ApplicationRecord".to_string()));
        assert_eq!(id_map.get(&308897), Some(&"Booth::archive!".to_string()));
    }

    #[test]
    fn test_parse_method_with_args() {
        let json = r#"{
            "id": 24,
            "name": { "kind": "CONSTANT", "name": "<root>" },
            "kind": "CLASS_OR_MODULE",
            "children": [
                {
                    "id": 100,
                    "name": { "kind": "CONSTANT", "name": "Campaign" },
                    "kind": "CLASS_OR_MODULE",
                    "isModule": false,
                    "superClass": 200,
                    "children": [
                        {
                            "id": 1001,
                            "name": { "kind": "UTF8", "name": "active?" },
                            "kind": "METHOD",
                            "arguments": [
                                { "name": { "kind": "UTF8", "name": "at" }, "isBlock": false },
                                { "name": { "kind": "UTF8", "name": "<blk>" }, "isBlock": true }
                            ]
                        }
                    ]
                }
            ]
        }"#;

        let tree = parse(json).unwrap();
        let campaign = &tree.root.children.as_ref().unwrap()[0];
        assert_eq!(campaign.is_module, Some(false));

        let method = &campaign.children.as_ref().unwrap()[0];
        assert_eq!(method.name.name, "active?");
        let args = method.arguments.as_ref().unwrap();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].name.name, "at");
        assert_eq!(args[0].is_block, Some(false));
        assert_eq!(args[1].name.name, "<blk>");
        assert_eq!(args[1].is_block, Some(true));
    }

    #[test]
    fn test_nested_modules() {
        let json = r#"{
            "id": 24,
            "name": { "kind": "CONSTANT", "name": "<root>" },
            "kind": "CLASS_OR_MODULE",
            "children": [
                {
                    "id": 100,
                    "name": { "kind": "CONSTANT", "name": "AdminArea" },
                    "kind": "CLASS_OR_MODULE",
                    "isModule": true,
                    "children": [
                        {
                            "id": 200,
                            "name": { "kind": "CONSTANT", "name": "CampaignsController" },
                            "kind": "CLASS_OR_MODULE",
                            "superClass": 300
                        }
                    ]
                }
            ]
        }"#;

        let tree = parse(json).unwrap();
        let id_map = tree.build_id_map();
        assert_eq!(id_map.get(&100), Some(&"AdminArea".to_string()));
        assert_eq!(
            id_map.get(&200),
            Some(&"AdminArea::CampaignsController".to_string())
        );
    }
}
