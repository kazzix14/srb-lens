use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde::{Serialize, Serializer};

use crate::builder::parse_sorbet_type;
use crate::parser::parse_tree::MethodLoc;

/// 3フォーマットを統合した全体像
#[derive(Debug, Default, Serialize)]
pub struct Project {
    pub classes: HashMap<String, ClassInfo>,
    pub methods: Vec<MethodInfo>,
}

impl Project {
    /// メソッド名でクエリ。"Foo#bar" or "Foo.bar" 形式。
    /// 部分一致でクラス名を検索する。
    /// 直接マッチしない場合、superclass チェーンを辿って親クラスのメソッドも探す。
    pub fn find_methods(&self, query: &str) -> Vec<&MethodInfo> {
        if let Some((class_part, method_part)) = query.split_once('#') {
            let direct: Vec<_> = self
                .methods
                .iter()
                .filter(|m| {
                    m.fqn.kind == MethodKind::Instance
                        && m.fqn.method_name == method_part
                        && m.fqn.class_fqn.ends_with(class_part)
                })
                .collect();
            if !direct.is_empty() {
                return direct;
            }
            self.find_inherited_method(class_part, method_part, MethodKind::Instance)
        } else if let Some((class_part, method_part)) = query.split_once('.') {
            let direct: Vec<_> = self
                .methods
                .iter()
                .filter(|m| {
                    m.fqn.kind == MethodKind::Class
                        && m.fqn.method_name == method_part
                        && m.fqn.class_fqn.ends_with(class_part)
                })
                .collect();
            if !direct.is_empty() {
                return direct;
            }
            self.find_inherited_method(class_part, method_part, MethodKind::Class)
        } else {
            // class name only — return all methods of matching classes
            self.methods
                .iter()
                .filter(|m| m.fqn.class_fqn.ends_with(query))
                .collect()
        }
    }

    /// superclass チェーンを辿って継承元のメソッドを探す
    fn find_inherited_method(
        &self,
        class_part: &str,
        method_name: &str,
        kind: MethodKind,
    ) -> Vec<&MethodInfo> {
        // class_part に部分一致するクラスを探す
        let matching_classes: Vec<_> = self
            .classes
            .values()
            .filter(|c| c.fqn.ends_with(class_part))
            .collect();

        for class in matching_classes {
            let mut current_super = class.super_class.as_deref();
            let mut visited = std::collections::HashSet::new();
            while let Some(super_fqn) = current_super {
                if !visited.insert(super_fqn.to_string()) {
                    break;
                }
                let results: Vec<_> = self
                    .methods
                    .iter()
                    .filter(|m| {
                        m.fqn.kind == kind
                            && m.fqn.method_name == method_name
                            && m.fqn.class_fqn == super_fqn
                    })
                    .collect();
                if !results.is_empty() {
                    return results;
                }
                current_super = self
                    .classes
                    .get(super_fqn)
                    .and_then(|c| c.super_class.as_deref());
            }
        }
        Vec::new()
    }

    /// クラス名でクエリ（部分一致）
    pub fn find_classes(&self, query: &str) -> Vec<&ClassInfo> {
        self.classes
            .values()
            .filter(|c| c.fqn.ends_with(query))
            .collect()
    }

    /// ソースファイルから各メソッドの定義位置を解決する
    pub fn resolve_source_locations(&mut self, project_root: &Path) {
        for method in &mut self.methods {
            let class_file = self
                .classes
                .get(&method.fqn.class_fqn)
                .and_then(|c| c.file_path.as_ref());
            let Some(rel_path) = class_file else {
                continue;
            };
            let full_path = project_root.join(rel_path);
            let Ok(content) = std::fs::read_to_string(&full_path) else {
                continue;
            };

            let def_pattern = match method.fqn.kind {
                MethodKind::Instance => format!("def {}", method.fqn.method_name),
                MethodKind::Class => format!("def self.{}", method.fqn.method_name),
            };

            for (i, line) in content.lines().enumerate() {
                if line.trim_start().starts_with(&def_pattern) {
                    method.file_path = Some(rel_path.clone());
                    method.line = Some(i + 1);
                    break;
                }
            }
        }
    }

    /// parse-tree-json-with-locs から抽出したメソッド位置を適用する
    pub fn resolve_source_locations_from_locs(&mut self, locs: &[MethodLoc]) {
        for method in &mut self.methods {
            let class_file = self
                .classes
                .get(&method.fqn.class_fqn)
                .and_then(|c| c.file_path.as_ref());
            let Some(rel_path) = class_file else {
                continue;
            };

            let is_class = method.fqn.kind == MethodKind::Class;
            // class の file_path に一致し、メソッド名と種類が一致する loc を探す
            // file_path が "./" で始まる場合を考慮して strip する
            let rel_normalized = rel_path.strip_prefix("./").unwrap_or(rel_path);
            if let Some(loc) = locs.iter().find(|l| {
                let loc_file = l.file.strip_prefix("./").unwrap_or(&l.file);
                loc_file == rel_normalized
                    && l.name == method.fqn.method_name
                    && l.is_class_method == is_class
            }) {
                method.file_path = Some(rel_path.clone());
                method.line = Some(loc.line);

                // sig の return type があれば CFG 推定より優先して適用
                if let Some(ref sig_ret) = loc.sig_return_type {
                    method.return_type = Some(parse_sorbet_type(sig_ret));
                }
            }
        }
    }
}

/// メソッドの完全修飾名
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MethodFqn {
    pub class_fqn: String,
    pub method_name: String,
    pub kind: MethodKind,
}

impl Serialize for MethodFqn {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl fmt::Display for MethodFqn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sep = match self.kind {
            MethodKind::Instance => "#",
            MethodKind::Class => ".",
        };
        write!(f, "{}{}{}", self.class_fqn, sep, self.method_name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum MethodKind {
    Instance,
    Class,
}

/// クラス/モジュール情報
#[derive(Debug, Clone, Serialize)]
pub struct ClassInfo {
    pub fqn: String,
    pub is_module: bool,
    pub super_class: Option<String>,
    pub mixins: Vec<String>,
    #[serde(skip)]
    pub method_fqns: Vec<MethodFqn>,
    pub file_path: Option<String>,
    pub line: Option<usize>,
}

/// メソッド情報
#[derive(Debug, Serialize)]
pub struct MethodInfo {
    pub fqn: MethodFqn,
    pub file_path: Option<String>,
    pub line: Option<usize>,
    pub arguments: Vec<Argument>,
    pub return_type: Option<SorbetType>,
    pub calls: Vec<MethodCall>,
    pub ivars: Vec<IvarAccess>,
    pub rescues: Vec<String>,
    pub uses_block: bool,
    #[serde(skip)]
    pub basic_blocks: Vec<BasicBlock>,
}

/// CFG の基本ブロック
#[derive(Debug)]
pub struct BasicBlock {
    pub id: usize,
    pub terminator: Terminator,
}

/// BB の終端（制御フローのエッジ）
#[derive(Debug)]
pub enum Terminator {
    /// 無条件ジャンプ
    Goto(usize),
    /// 条件分岐
    Branch {
        condition: String,
        true_bb: usize,
        false_bb: usize,
    },
    /// ブロック呼び出し (yield)
    BlockCall { true_bb: usize, false_bb: usize },
    /// メソッドからの return
    Return,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArgumentKind {
    Req,
    Opt,
    Rest,
    KeyReq,
    Key,
    KeyRest,
    Block,
}

#[derive(Debug, Clone, Serialize)]
pub struct Argument {
    pub name: String,
    pub ty: SorbetType,
    pub kind: ArgumentKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct MethodCall {
    pub receiver_type: SorbetType,
    pub method_name: String,
    pub return_type: SorbetType,
    pub bb_id: usize,
    pub conditions: Vec<BranchCondition>,
}

/// ある呼び出しに到達するために通った分岐条件
#[derive(Debug, Clone, Serialize)]
pub struct BranchCondition {
    pub call: String,
    pub is_true: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct IvarAccess {
    pub name: String,
    pub ty: SorbetType,
}

/// Sorbet の型表記
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SorbetType {
    Simple(String),
    Nilable(Box<SorbetType>),
    Union(Vec<SorbetType>),
    Array(Box<SorbetType>),
    Hash(Box<SorbetType>, Box<SorbetType>),
    ClassOf(String),
    Tuple(Vec<SorbetType>),
    Shape(Vec<(String, SorbetType)>),
    Literal(String),
    Boolean,
    Untyped,
    NoReturn,
    Void,
}

impl Serialize for SorbetType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl fmt::Display for SorbetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SorbetType::Simple(s) => write!(f, "{s}"),
            SorbetType::Nilable(inner) => write!(f, "T.nilable({inner})"),
            SorbetType::Union(types) => {
                let parts: Vec<_> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "T.any({})", parts.join(", "))
            }
            SorbetType::Array(inner) => write!(f, "T::Array[{inner}]"),
            SorbetType::Hash(k, v) => write!(f, "T::Hash[{k}, {v}]"),
            SorbetType::ClassOf(name) => write!(f, "T.class_of({name})"),
            SorbetType::Tuple(types) => {
                let parts: Vec<_> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "[{}]", parts.join(", "))
            }
            SorbetType::Shape(fields) => {
                let parts: Vec<_> = fields.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                write!(f, "{{{}}}", parts.join(", "))
            }
            SorbetType::Literal(s) => write!(f, "{s}"),
            SorbetType::Boolean => write!(f, "T::Boolean"),
            SorbetType::Untyped => write!(f, "T.untyped"),
            SorbetType::NoReturn => write!(f, "T.noreturn"),
            SorbetType::Void => write!(f, "void"),
        }
    }
}
