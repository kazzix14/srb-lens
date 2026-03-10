use std::collections::{HashMap, HashSet};

use crate::model::*;
use crate::parser::autogen::AutogenFile;
use crate::parser::cfg_text::{AliasKind, CfgInstruction, CfgMethod, CfgTerminator};
use crate::parser::symbol_table::SymbolTree;

pub fn build(
    cfg_methods: Vec<CfgMethod>,
    symbol_tree: SymbolTree,
    autogen_files: Vec<AutogenFile>,
) -> Project {
    let mut project = Project::default();

    // 1. symbol-table → クラス情報
    let id_map = symbol_tree.build_id_map();
    build_classes_from_symbols(&symbol_tree.root, "", &id_map, &mut project);

    // 2. autogen → ファイルパスと行番号をクラスに紐付け
    apply_autogen(&autogen_files, &mut project);

    // 3. cfg-text → メソッド情報
    for cfg_method in &cfg_methods {
        if let Some(method_info) = extract_method_info(cfg_method) {
            // クラスの method_fqns にも追加
            if let Some(class) = project.classes.get_mut(&method_info.fqn.class_fqn) {
                class.method_fqns.push(method_info.fqn.clone());
            }
            project.methods.push(method_info);
        }
    }

    project
}

fn build_classes_from_symbols(
    symbol: &crate::parser::symbol_table::RawSymbol,
    parent_fqn: &str,
    id_map: &HashMap<u64, String>,
    project: &mut Project,
) {
    if symbol.kind == "CLASS_OR_MODULE" && symbol.name.name != "<root>" {
        let fqn = if parent_fqn.is_empty() {
            symbol.name.name.clone()
        } else {
            format!("{parent_fqn}::{}", symbol.name.name)
        };

        let super_class = symbol
            .super_class
            .and_then(|id| id_map.get(&id))
            .cloned();

        let mixins = symbol
            .mixins
            .as_ref()
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| id_map.get(id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        project.classes.insert(
            fqn.clone(),
            ClassInfo {
                fqn: fqn.clone(),
                is_module: symbol.is_module.unwrap_or(false),
                super_class,
                mixins,
                method_fqns: Vec::new(),
                file_path: None,
                line: None,
            },
        );

        if let Some(children) = &symbol.children {
            for child in children {
                build_classes_from_symbols(child, &fqn, id_map, project);
            }
        }
    } else if let Some(children) = &symbol.children {
        let current_fqn = if symbol.name.name == "<root>" {
            ""
        } else {
            parent_fqn
        };
        for child in children {
            build_classes_from_symbols(child, current_fqn, id_map, project);
        }
    }
}

fn is_rbi_path(path: &str) -> bool {
    path.contains("sorbet/rbi/") || path.ends_with(".rbi")
}

/// FQN とファイルパスがどれくらいマッチするかスコア化。
/// Campaign → campaign.rb (高), campaign/variable.rb (低)
fn path_match_score(fqn: &str, path: &str) -> u32 {
    if is_rbi_path(path) {
        return 0;
    }
    // FQN "Campaign::Variable" → expected stem "campaign/variable"
    let expected = fqn.replace("::", "/").to_lowercase();
    let path_lower = path.to_lowercase();
    // path が /campaign/variable.rb で終わるか
    if path_lower.ends_with(&format!("/{expected}.rb")) || path_lower == format!("{expected}.rb") {
        2 // 完全一致
    } else {
        1 // 非RBIだが完全一致ではない
    }
}

fn apply_autogen(autogen_files: &[AutogenFile], project: &mut Project) {
    for file in autogen_files {
        for r in &file.refs {
            if r.is_defining_ref {
                let fqn = r.resolved.join("::");
                if let Some(class) = project.classes.get_mut(&fqn) {
                    let new_score = path_match_score(&fqn, &file.path);
                    let existing_score = class
                        .file_path
                        .as_ref()
                        .map(|p| path_match_score(&fqn, p))
                        .unwrap_or(0);

                    if new_score > existing_score {
                        class.file_path = Some(file.path.clone());
                        if let Some(line) =
                            r.loc.rsplit_once(':').and_then(|(_, l)| l.parse().ok())
                        {
                            class.line = Some(line);
                        }
                    }
                }
            }
        }
    }
}

/// cfg-text のメソッド名をパースして MethodFqn にする
fn parse_method_fqn(raw_name: &str) -> Option<MethodFqn> {
    // "::Campaign#active?" → Instance("Campaign", "active?")
    // "::DynamoDB::<Class:Code>#decode_counter_from_code" → Class("DynamoDB::Code", "decode...")
    // "::<Class:<root>>#<static-init>" → skip

    let name = raw_name.strip_prefix("::")?;

    // skip static-init and other internal methods
    if name.contains("<root>") || name.contains("<static-init>") {
        return None;
    }

    let hash_pos = name.find('#')?;
    let class_part = &name[..hash_pos];
    let method_name = name[hash_pos + 1..].to_string();

    // Check for class method: <Class:ClassName>
    if let Some(inner) = class_part.strip_suffix('>') {
        if let Some(class_start) = inner.rfind("<Class:") {
            let actual_class = &inner[class_start + 7..];
            // Reconstruct FQN: everything before <Class:...> + the class name
            let prefix = &inner[..class_start];
            let class_fqn = if prefix.is_empty() {
                actual_class.to_string()
            } else {
                let prefix = prefix.strip_suffix("::").unwrap_or(prefix);
                format!("{prefix}::{actual_class}")
            };
            return Some(MethodFqn {
                class_fqn,
                method_name,
                kind: MethodKind::Class,
            });
        }
    }

    Some(MethodFqn {
        class_fqn: class_part.to_string(),
        method_name,
        kind: MethodKind::Instance,
    })
}

fn extract_method_info(cfg_method: &CfgMethod) -> Option<MethodInfo> {
    let fqn = parse_method_fqn(&cfg_method.raw_name)?;

    let mut arguments = Vec::new();
    let mut calls = Vec::new();
    let mut ivars = Vec::new();
    let mut rescues = Vec::new();
    let mut uses_block = false;
    let mut return_types: Vec<SorbetType> = Vec::new();
    let mut optional_args: HashSet<String> = HashSet::new();

    for block in &cfg_method.blocks {
        for instr in &block.instructions {
            match instr {
                CfgInstruction::LoadArg { lhs, arg_name } => {
                    arguments.push(Argument {
                        name: arg_name.clone(),
                        ty: parse_sorbet_type(&lhs.ty),
                        is_optional: false, // updated later
                    });
                }
                CfgInstruction::ArgPresent { arg_name, .. } => {
                    optional_args.insert(arg_name.clone());
                }
                CfgInstruction::MethodCall {
                    lhs,
                    receiver,
                    method_name,
                    ..
                } => {
                    // skip internal Magic methods like <build-hash>, <expand-splat>
                    if !is_magic_method(method_name, &receiver.ty) {
                        calls.push(MethodCall {
                            receiver_type: parse_sorbet_type(&receiver.ty),
                            method_name: method_name.clone(),
                            return_type: parse_sorbet_type(&lhs.ty),
                            bb_id: block.id,
                            conditions: Vec::new(), // filled in later
                        });
                    }
                }
                CfgInstruction::Alias { kind, lhs, .. } => {
                    if let AliasKind::Ivar(name) = kind {
                        ivars.push(IvarAccess {
                            name: name.clone(),
                            ty: parse_sorbet_type(&lhs.ty),
                        });
                    }
                }
                CfgInstruction::Return { value } => {
                    let ty = parse_sorbet_type(&value.ty);
                    if !return_types.contains(&ty) {
                        return_types.push(ty);
                    }
                }
                CfgInstruction::GetCurrentException { .. } => {
                    // rescue detected; actual exception types found via isa checks
                }
                _ => {}
            }
        }

        // Detect rescue exception types from isa checks in conditional branches
        if let CfgTerminator::Conditional { .. } = &block.terminator {
            for instr in &block.instructions {
                if let CfgInstruction::MethodCall {
                    method_name,
                    receiver,
                    ..
                } = instr
                {
                    if method_name == "===" && receiver.ty.starts_with("T.class_of(") {
                        let inner = receiver
                            .ty
                            .strip_prefix("T.class_of(")
                            .and_then(|s| s.strip_suffix(')'));
                        if let Some(exception_class) = inner {
                            rescues.push(exception_class.to_string());
                        }
                    }
                }
            }
        }

        // Detect block usage
        if matches!(&block.terminator, CfgTerminator::BlockCall { .. }) {
            uses_block = true;
        }
        for instr in &block.instructions {
            if matches!(instr, CfgInstruction::LoadSelf { .. } | CfgInstruction::LoadYieldParams { .. }) {
                uses_block = true;
            }
        }
    }

    // Mark optional arguments
    for arg in &mut arguments {
        if optional_args.contains(&arg.name) {
            arg.is_optional = true;
        }
    }

    // Deduplicate ivars by name
    ivars.sort_by(|a, b| a.name.cmp(&b.name));
    ivars.dedup_by(|a, b| a.name == b.name);

    // Build basic block graph
    let basic_blocks = build_basic_blocks(cfg_method);

    // Compute conditions for each call
    let predecessors = build_predecessor_map(&basic_blocks);
    for call in &mut calls {
        call.conditions = compute_conditions(call.bb_id, &basic_blocks, &predecessors);
    }

    let return_type = if return_types.is_empty() {
        None
    } else if return_types.contains(&SorbetType::Untyped) {
        Some(SorbetType::Untyped)
    } else if return_types.len() == 1 {
        Some(return_types.remove(0))
    } else {
        Some(SorbetType::Union(return_types))
    };

    Some(MethodInfo {
        fqn,
        file_path: None,
        line: None,
        arguments,
        return_type,
        calls,
        ivars,
        rescues,
        uses_block,
        basic_blocks,
    })
}

/// CFG の基本ブロックからグラフ構造を構築（自己ループの dead block は除外）
fn build_basic_blocks(cfg_method: &CfgMethod) -> Vec<BasicBlock> {
    cfg_method
        .blocks
        .iter()
        .filter(|block| {
            // 自己ループ（dead block）を除外
            !matches!(&block.terminator, CfgTerminator::Unconditional { target } if *target == block.id)
        })
        .map(|block| {
            let has_return = block
                .instructions
                .iter()
                .any(|i| matches!(i, CfgInstruction::Return { .. }));

            let terminator = if has_return {
                Terminator::Return
            } else {
                match &block.terminator {
                    CfgTerminator::Unconditional { target } => Terminator::Goto(*target),
                    CfgTerminator::Conditional {
                        var,
                        true_bb,
                        false_bb,
                    } => {
                        let condition = resolve_condition(
                            &cfg_method.blocks,
                            &var.name,
                            block.id,
                            &mut HashSet::new(),
                        );
                        Terminator::Branch {
                            condition,
                            true_bb: *true_bb,
                            false_bb: *false_bb,
                        }
                    }
                    CfgTerminator::BlockCall { true_bb, false_bb } => {
                        Terminator::BlockCall {
                            true_bb: *true_bb,
                            false_bb: *false_bb,
                        }
                    }
                }
            };

            BasicBlock {
                id: block.id,
                terminator,
            }
        })
        .collect()
}

/// 条件分岐の変数を解決して人間が読める文字列にする。
/// 現在の BB を優先的に検索し、Assignment/Cast はソース変数を辿る。
fn resolve_condition(
    all_blocks: &[crate::parser::cfg_text::CfgBasicBlock],
    var_name: &str,
    start_block_id: usize,
    visited: &mut HashSet<String>,
) -> String {
    if !visited.insert(var_name.to_string()) {
        return var_name.to_string();
    }

    // 現在の BB を先頭にした検索順序
    let mut block_order: Vec<_> = Vec::with_capacity(all_blocks.len());
    if let Some(start) = all_blocks.iter().find(|b| b.id == start_block_id) {
        block_order.push(start);
    }
    for block in all_blocks {
        if block.id != start_block_id {
            block_order.push(block);
        }
    }

    for block in block_order {
        for instr in block.instructions.iter().rev() {
            match instr {
                CfgInstruction::MethodCall {
                    lhs,
                    receiver,
                    method_name,
                    ..
                } => {
                    if lhs.name == var_name && !is_magic_method(method_name, &receiver.ty) {
                        return format!("{}.{}() -> {}", receiver.ty, method_name, lhs.ty);
                    }
                }
                CfgInstruction::ArgPresent { lhs, arg_name } => {
                    if lhs.name == var_name {
                        return format!("arg_present({})", arg_name);
                    }
                }
                CfgInstruction::Cast { lhs, source, .. } => {
                    if lhs.name == var_name {
                        return resolve_condition(all_blocks, &source.name, block.id, visited);
                    }
                }
                CfgInstruction::Assignment { lhs, rhs } => {
                    if lhs.name == var_name {
                        // rhs は "$7: T::Boolean" のような形式。変数名部分を取り出す
                        let source_name = rhs.split(':').next().unwrap_or(rhs).trim();
                        if !source_name.is_empty() {
                            return resolve_condition(
                                all_blocks,
                                source_name,
                                block.id,
                                visited,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }
    var_name.to_string()
}

/// BB の predecessor マップを構築: bb_id → [(predecessor_bb_id, branch_direction)]
/// branch_direction: Some(true) = true分岐, Some(false) = false分岐, None = 無条件
fn build_predecessor_map(basic_blocks: &[BasicBlock]) -> HashMap<usize, Vec<(usize, Option<bool>)>> {
    let mut preds: HashMap<usize, Vec<(usize, Option<bool>)>> = HashMap::new();
    for bb in basic_blocks {
        match &bb.terminator {
            Terminator::Goto(target) => {
                preds.entry(*target).or_default().push((bb.id, None));
            }
            Terminator::Branch {
                true_bb, false_bb, ..
            } => {
                preds.entry(*true_bb).or_default().push((bb.id, Some(true)));
                preds
                    .entry(*false_bb)
                    .or_default()
                    .push((bb.id, Some(false)));
            }
            Terminator::BlockCall {
                true_bb, false_bb, ..
            } => {
                preds.entry(*true_bb).or_default().push((bb.id, Some(true)));
                preds
                    .entry(*false_bb)
                    .or_default()
                    .push((bb.id, Some(false)));
            }
            Terminator::Return => {}
        }
    }
    preds
}

/// あるBBに到達するまでの分岐条件を逆向きにたどって収集
fn compute_conditions(
    bb_id: usize,
    basic_blocks: &[BasicBlock],
    predecessors: &HashMap<usize, Vec<(usize, Option<bool>)>>,
) -> Vec<BranchCondition> {
    let mut conditions = Vec::new();
    let mut current = bb_id;
    let mut visited = HashSet::new();

    loop {
        if !visited.insert(current) {
            break; // ループ検出
        }
        let preds = match predecessors.get(&current) {
            Some(p) if p.len() == 1 => p,
            _ => break, // エントリ or 合流点
        };

        let (pred_bb, direction) = &preds[0];
        if let Some(is_true) = direction {
            // 条件分岐のエッジ → condition を記録
            if let Some(pred_block) = basic_blocks.iter().find(|b| b.id == *pred_bb) {
                if let Terminator::Branch { condition, .. } = &pred_block.terminator {
                    conditions.push(BranchCondition {
                        call: condition.clone(),
                        is_true: *is_true,
                    });
                }
            }
        }
        current = *pred_bb;
    }

    conditions.reverse(); // エントリ側から順に
    conditions
}

/// Magic 内部メソッドかどうか判定
fn is_magic_method(method_name: &str, receiver_type: &str) -> bool {
    receiver_type.contains("<Magic>")
        || (method_name.starts_with('<') && method_name.contains('-'))
}

fn parse_sorbet_type(s: &str) -> SorbetType {
    let s = s.trim();
    if s.is_empty() || s == "T.untyped" {
        return SorbetType::Untyped;
    }
    if s == "T.noreturn" {
        return SorbetType::NoReturn;
    }
    if s == "T::Boolean" {
        return SorbetType::Boolean;
    }
    if s == "void" {
        return SorbetType::Void;
    }

    if let Some(inner) = s.strip_prefix("T.nilable(").and_then(|s| s.strip_suffix(')')) {
        return SorbetType::Nilable(Box::new(parse_sorbet_type(inner)));
    }
    if let Some(inner) = s.strip_prefix("T.any(").and_then(|s| s.strip_suffix(')')) {
        let parts = split_type_args(inner);
        return SorbetType::Union(parts.into_iter().map(|p| parse_sorbet_type(p)).collect());
    }
    if let Some(inner) = s.strip_prefix("T::Array[").and_then(|s| s.strip_suffix(']')) {
        return SorbetType::Array(Box::new(parse_sorbet_type(inner)));
    }
    if let Some(inner) = s.strip_prefix("T::Hash[").and_then(|s| s.strip_suffix(']')) {
        let parts = split_type_args(inner);
        if parts.len() == 2 {
            return SorbetType::Hash(
                Box::new(parse_sorbet_type(parts[0])),
                Box::new(parse_sorbet_type(parts[1])),
            );
        }
    }
    if let Some(inner) = s.strip_prefix("T.class_of(").and_then(|s| s.strip_suffix(')')) {
        return SorbetType::ClassOf(inner.to_string());
    }

    // Tuple: [X, Y]
    if s.starts_with('[') && s.ends_with(']') {
        let inner = &s[1..s.len() - 1];
        let parts = split_type_args(inner);
        return SorbetType::Tuple(parts.into_iter().map(|p| parse_sorbet_type(p)).collect());
    }

    // Shape: {key: Type}
    if s.starts_with('{') && s.ends_with('}') {
        let inner = &s[1..s.len() - 1];
        let fields: Vec<_> = split_type_args(inner)
            .into_iter()
            .filter_map(|part| {
                let colon = part.find(": ")?;
                Some((
                    part[..colon].to_string(),
                    parse_sorbet_type(&part[colon + 2..]),
                ))
            })
            .collect();
        return SorbetType::Shape(fields);
    }

    // Literal types: Integer(2), Symbol(:name), String("...")
    if (s.starts_with("Integer(")
        || s.starts_with("Symbol(")
        || s.starts_with("String("))
        && s.ends_with(')')
    {
        return SorbetType::Literal(s.to_string());
    }

    SorbetType::Simple(s.to_string())
}

/// Split comma-separated type arguments respecting nesting
fn split_type_args(s: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut start = 0;

    for (i, b) in s.bytes().enumerate() {
        match b {
            b'(' | b'[' | b'<' | b'{' => depth += 1,
            b')' | b']' | b'>' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                result.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    result.push(s[start..].trim());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{autogen, cfg_text, symbol_table};

    #[test]
    fn test_parse_method_fqn_instance() {
        let fqn = parse_method_fqn("::Campaign#active?").unwrap();
        assert_eq!(fqn.class_fqn, "Campaign");
        assert_eq!(fqn.method_name, "active?");
        assert_eq!(fqn.kind, MethodKind::Instance);
    }

    #[test]
    fn test_parse_method_fqn_class_method() {
        let fqn = parse_method_fqn("::DynamoDB::<Class:Code>#decode_counter_from_code").unwrap();
        assert_eq!(fqn.class_fqn, "DynamoDB::Code");
        assert_eq!(fqn.method_name, "decode_counter_from_code");
        assert_eq!(fqn.kind, MethodKind::Class);
    }

    #[test]
    fn test_parse_method_fqn_nested() {
        let fqn = parse_method_fqn("::AdminArea::CampaignsController#index").unwrap();
        assert_eq!(fqn.class_fqn, "AdminArea::CampaignsController");
        assert_eq!(fqn.method_name, "index");
        assert_eq!(fqn.kind, MethodKind::Instance);
    }

    #[test]
    fn test_parse_method_fqn_skip_static_init() {
        assert!(parse_method_fqn("::<Class:<root>>#<static-init>").is_none());
    }

    #[test]
    fn test_parse_sorbet_type() {
        assert_eq!(parse_sorbet_type("String"), SorbetType::Simple("String".into()));
        assert_eq!(parse_sorbet_type("T.untyped"), SorbetType::Untyped);
        assert_eq!(parse_sorbet_type("T::Boolean"), SorbetType::Boolean);
        assert_eq!(
            parse_sorbet_type("T.nilable(String)"),
            SorbetType::Nilable(Box::new(SorbetType::Simple("String".into())))
        );
        assert_eq!(
            parse_sorbet_type("T::Array[Integer]"),
            SorbetType::Array(Box::new(SorbetType::Simple("Integer".into())))
        );
        assert_eq!(
            parse_sorbet_type("T::Hash[String, Integer]"),
            SorbetType::Hash(
                Box::new(SorbetType::Simple("String".into())),
                Box::new(SorbetType::Simple("Integer".into()))
            )
        );
    }

    #[test]
    fn test_build_integration() {
        let cfg_input = r#"method ::Campaign#active? {

bb0[firstDead=-1]():
    <self>: Campaign = cast(<self>: NilClass, Campaign);
    at: Time = load_arg(at)
    <argPresent>$3: T::Boolean = arg_present(at)
    @start_at$4: T.untyped = alias @start_at
    <statTemp>$5: Time = <self>: Campaign.start_at()
    <returnMethodTemp>$2: T::Boolean = <statTemp>$5: Time.<=>(at: Time)
    <finalReturn>: T.noreturn = return <returnMethodTemp>$2: T::Boolean
    <unconditional> -> bb1

# backedges
# - bb0
bb1[firstDead=-1]():
    <unconditional> -> bb1

}"#;

        let symbol_json = r#"{
            "id": 24,
            "name": { "kind": "CONSTANT", "name": "<root>" },
            "kind": "CLASS_OR_MODULE",
            "children": [
                {
                    "id": 100,
                    "name": { "kind": "CONSTANT", "name": "Campaign" },
                    "kind": "CLASS_OR_MODULE",
                    "superClass": 200,
                    "isModule": false,
                    "children": []
                },
                {
                    "id": 200,
                    "name": { "kind": "CONSTANT", "name": "ApplicationRecord" },
                    "kind": "CLASS_OR_MODULE"
                }
            ]
        }"#;

        let autogen_input = r#"# ParsedFile: ./app/models/campaign.rb
requires: []
## defs:
[def id=0]
 type=class
 defines_behavior=1
 is_empty=0
 defining_ref=[Campaign]
## refs:
[ref id=0]
 scope=[]
 name=[Campaign]
 nesting=[]
 resolved=[Campaign]
 loc=app/models/campaign.rb:3
 is_defining_ref=1"#;

        let cfg_methods = cfg_text::parse(cfg_input).unwrap();
        let symbol_tree = symbol_table::parse(symbol_json).unwrap();
        let autogen_files = autogen::parse(autogen_input).unwrap();

        let project = build(cfg_methods, symbol_tree, autogen_files);

        // Class info
        let campaign = project.classes.get("Campaign").unwrap();
        assert_eq!(campaign.super_class.as_deref(), Some("ApplicationRecord"));
        assert_eq!(campaign.file_path.as_deref(), Some("./app/models/campaign.rb"));
        assert_eq!(campaign.line, Some(3));

        // Method info
        let methods = project.find_methods("Campaign#active?");
        assert_eq!(methods.len(), 1);
        let m = &methods[0];
        assert_eq!(m.fqn.to_string(), "Campaign#active?");
        assert_eq!(m.arguments.len(), 1);
        assert_eq!(m.arguments[0].name, "at");
        assert!(m.arguments[0].is_optional);
        assert_eq!(m.ivars.len(), 1);
        assert_eq!(m.ivars[0].name, "@start_at");
        assert!(m.calls.iter().any(|c| c.method_name == "start_at"));
        assert!(m.calls.iter().any(|c| c.method_name == "<=>"));
    }
}
