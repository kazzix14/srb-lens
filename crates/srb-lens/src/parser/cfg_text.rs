#[derive(Debug)]
pub struct CfgMethod {
    pub raw_name: String,
    pub blocks: Vec<CfgBasicBlock>,
}

#[derive(Debug)]
pub struct CfgBasicBlock {
    pub id: usize,
    pub first_dead: i32,
    pub params: Vec<CfgVar>,
    pub instructions: Vec<CfgInstruction>,
    pub terminator: CfgTerminator,
    pub backedges: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct CfgVar {
    pub name: String,
    pub ty: String,
}

#[derive(Debug)]
pub enum CfgInstruction {
    Cast {
        lhs: CfgVar,
        source: CfgVar,
        target_type: String,
    },
    Alias {
        lhs: CfgVar,
        kind: AliasKind,
    },
    LoadArg {
        lhs: CfgVar,
        arg_name: String,
    },
    ArgPresent {
        lhs: CfgVar,
        arg_name: String,
    },
    MethodCall {
        lhs: CfgVar,
        receiver: CfgVar,
        method_name: String,
        args: Vec<CfgVar>,
    },
    Return {
        value: CfgVar,
    },
    BlockReturn {
        block_name: String,
        value: CfgVar,
    },
    Literal {
        lhs: CfgVar,
        value: String,
    },
    Assignment {
        lhs: CfgVar,
        rhs: String,
    },
    GetCurrentException {
        lhs: CfgVar,
    },
    LoadSelf {
        lhs: CfgVar,
        block_name: String,
    },
    LoadYieldParams {
        lhs: CfgVar,
        block_name: String,
    },
    YieldLoadArg {
        lhs: CfgVar,
        index: usize,
        source: CfgVar,
    },
    Solve {
        lhs: CfgVar,
        temp: String,
        block_name: String,
    },
    KeepAlive {
        lhs: CfgVar,
        var: String,
    },
}

#[derive(Debug)]
pub enum AliasKind {
    Constant(String),
    Ivar(String),
}

#[derive(Debug)]
pub enum CfgTerminator {
    Unconditional { target: usize },
    Conditional { var: CfgVar, true_bb: usize, false_bb: usize },
    BlockCall { true_bb: usize, false_bb: usize },
}

#[derive(Debug, thiserror::Error)]
pub enum CfgParseError {
    #[error("parse error at line {line}: {message}")]
    ParseError { line: usize, message: String },
}

pub fn parse(input: &str) -> Result<Vec<CfgMethod>, CfgParseError> {
    let mut methods = Vec::new();
    let mut lines = input.lines().enumerate().peekable();

    while let Some((line_no, line)) = lines.next() {
        if let Some(name) = line.strip_prefix("method ") {
            let name = name.strip_suffix(" {").unwrap_or(name).to_string();
            let blocks = parse_method_body(&mut lines)?;
            methods.push(CfgMethod { raw_name: name, blocks });
        } else if !line.is_empty() && line != "}" {
            // skip blank lines and closing braces between methods
            continue;
        }
        let _ = line_no; // used for error context if needed
    }

    Ok(methods)
}

fn parse_method_body(
    lines: &mut std::iter::Peekable<std::iter::Enumerate<std::str::Lines<'_>>>,
) -> Result<Vec<CfgBasicBlock>, CfgParseError> {
    let mut blocks = Vec::new();
    let mut current_bb: Option<BbBuilder> = None;
    // backedges appear before the BB header they belong to, so buffer them
    let mut pending_backedges: Vec<usize> = Vec::new();

    while let Some(&(line_no, line)) = lines.peek() {
        if line == "}" {
            // end of method
            if let Some(bb) = current_bb.take() {
                blocks.push(bb.finish());
            }
            lines.next();
            return Ok(blocks);
        }

        lines.next();

        if line.is_empty() || line.starts_with("    # outerLoops:") {
            continue;
        }

        // BB header: bb<N>[firstDead=<M>](<params>):
        if line.starts_with("bb") && line.contains("[firstDead=") {
            if let Some(bb) = current_bb.take() {
                blocks.push(bb.finish());
            }
            let mut bb = parse_bb_header(line, line_no)?;
            bb.backedges = std::mem::take(&mut pending_backedges);
            current_bb = Some(bb);
            continue;
        }

        // backedges (belong to the NEXT BB)
        if line == "# backedges" {
            continue;
        }
        if let Some(rest) = line.strip_prefix("# - bb") {
            if let Ok(id) = rest.parse::<usize>() {
                pending_backedges.push(id);
            }
            continue;
        }

        // instruction lines (4-space indented)
        if let Some(instr_line) = line.strip_prefix("    ") {
            if let Some(bb) = &mut current_bb {
                parse_instruction_line(instr_line, line_no, bb)?;
            }
            continue;
        }
    }

    if let Some(bb) = current_bb {
        blocks.push(bb.finish());
    }
    Ok(blocks)
}

struct BbBuilder {
    id: usize,
    first_dead: i32,
    params: Vec<CfgVar>,
    instructions: Vec<CfgInstruction>,
    terminator: Option<CfgTerminator>,
    backedges: Vec<usize>,
}

impl BbBuilder {
    fn finish(self) -> CfgBasicBlock {
        CfgBasicBlock {
            id: self.id,
            first_dead: self.first_dead,
            params: self.params,
            instructions: self.instructions,
            terminator: self.terminator.unwrap_or(CfgTerminator::Unconditional { target: 0 }),
            backedges: self.backedges,
        }
    }
}

fn parse_bb_header(line: &str, line_no: usize) -> Result<BbBuilder, CfgParseError> {
    // bb<N>[firstDead=<M>](<params>):
    let err = || CfgParseError::ParseError {
        line: line_no + 1,
        message: format!("invalid BB header: {line}"),
    };

    let rest = line.strip_prefix("bb").ok_or_else(err)?;
    let bracket_pos = rest.find('[').ok_or_else(err)?;
    let id: usize = rest[..bracket_pos].parse().map_err(|_| err())?;

    let first_dead_start = rest.find("firstDead=").ok_or_else(err)? + "firstDead=".len();
    let first_dead_end = rest[first_dead_start..].find(']').ok_or_else(err)? + first_dead_start;
    let first_dead: i32 = rest[first_dead_start..first_dead_end].parse().map_err(|_| err())?;

    // parse params between ( and ):
    let params = if let Some(paren_start) = rest.find('(') {
        let paren_end = rest.rfind(')').ok_or_else(err)?;
        let params_str = &rest[paren_start + 1..paren_end];
        if params_str.is_empty() {
            Vec::new()
        } else {
            parse_typed_var_list(params_str)
        }
    } else {
        Vec::new()
    };

    Ok(BbBuilder {
        id,
        first_dead,
        params,
        instructions: Vec::new(),
        terminator: None,
        backedges: Vec::new(),
    })
}

fn parse_instruction_line(
    line: &str,
    _line_no: usize,
    bb: &mut BbBuilder,
) -> Result<(), CfgParseError> {
    // Terminator: <unconditional> -> bb<N>
    if let Some(rest) = line.strip_prefix("<unconditional> -> bb") {
        let target: usize = rest.parse().unwrap_or(0);
        bb.terminator = Some(CfgTerminator::Unconditional { target });
        return Ok(());
    }

    // Terminator: <block-call> -> (NilClass ? bb<N> : bb<M>)
    if line.starts_with("<block-call> -> ") {
        if let Some((true_bb, false_bb)) = parse_branch_targets(line) {
            bb.terminator = Some(CfgTerminator::BlockCall { true_bb, false_bb });
        }
        return Ok(());
    }

    // Terminator: <var> -> (<Type> ? bb<N> : bb<M>)
    if line.contains(" -> (") && line.contains(" ? bb") {
        let arrow_pos = line.find(" -> (").unwrap();
        let var_str = &line[..arrow_pos];
        let var = parse_typed_var_or_name(var_str);
        if let Some((true_bb, false_bb)) = parse_branch_targets(line) {
            bb.terminator = Some(CfgTerminator::Conditional { var, true_bb, false_bb });
        }
        return Ok(());
    }

    // Instructions with " = " assignment
    if let Some(eq_pos) = find_top_level_eq(line) {
        let lhs_str = &line[..eq_pos];
        let rhs_str = &line[eq_pos + 3..]; // skip " = "

        let lhs = parse_typed_var(lhs_str);

        // cast(<source>: <Type>, <TargetType>);
        if let Some(cast_inner) = rhs_str.strip_prefix("cast(") {
            let cast_inner = cast_inner.strip_suffix(");").unwrap_or(cast_inner);
            if let Some(comma_pos) = find_last_comma_top_level(cast_inner) {
                let source = parse_typed_var(&cast_inner[..comma_pos]);
                let target_type = cast_inner[comma_pos + 2..].to_string();
                bb.instructions.push(CfgInstruction::Cast { lhs, source, target_type });
            }
            return Ok(());
        }

        // alias <C ConstName> or alias @ivar or alias <C <undeclared-field-stub>> (@ivar)
        if rhs_str.starts_with("alias ") {
            let alias_rest = &rhs_str[6..];
            let kind = if alias_rest.starts_with("<C ") {
                // alias <C ConstName> or alias <C <undeclared-field-stub>> (@ivar)
                if alias_rest.contains("(@") {
                    // alias <C <undeclared-field-stub>> (@campaigns)
                    let paren_start = alias_rest.find("(@").unwrap();
                    let paren_end = alias_rest.rfind(')').unwrap();
                    let ivar = alias_rest[paren_start + 1..paren_end].to_string();
                    AliasKind::Ivar(ivar)
                } else {
                    let end = alias_rest.rfind('>').unwrap_or(alias_rest.len());
                    let const_name = alias_rest[3..end].to_string();
                    AliasKind::Constant(const_name)
                }
            } else if alias_rest.starts_with('@') {
                AliasKind::Ivar(alias_rest.to_string())
            } else {
                AliasKind::Constant(alias_rest.to_string())
            };
            bb.instructions.push(CfgInstruction::Alias { lhs, kind });
            return Ok(());
        }

        // load_arg(name)
        if let Some(inner) = rhs_str.strip_prefix("load_arg(") {
            let arg_name = inner.strip_suffix(')').unwrap_or(inner).to_string();
            bb.instructions.push(CfgInstruction::LoadArg { lhs, arg_name });
            return Ok(());
        }

        // arg_present(name)
        if let Some(inner) = rhs_str.strip_prefix("arg_present(") {
            let arg_name = inner.strip_suffix(')').unwrap_or(inner).to_string();
            bb.instructions.push(CfgInstruction::ArgPresent { lhs, arg_name });
            return Ok(());
        }

        // return <var>: <Type>
        if rhs_str.starts_with("return ") {
            let value = parse_typed_var(&rhs_str[7..]);
            bb.instructions.push(CfgInstruction::Return { value });
            return Ok(());
        }

        // blockreturn<name> <var>
        if rhs_str.starts_with("blockreturn<") {
            let end_angle = rhs_str.find('>').unwrap_or(rhs_str.len());
            let block_name = rhs_str[12..end_angle].to_string();
            let value_str = &rhs_str[end_angle + 2..]; // skip "> "
            let value = parse_typed_var_or_name(value_str);
            bb.instructions.push(CfgInstruction::BlockReturn { block_name, value });
            return Ok(());
        }

        // <get-current-exception>
        if rhs_str == "<get-current-exception>" {
            bb.instructions.push(CfgInstruction::GetCurrentException { lhs });
            return Ok(());
        }

        // loadSelf(block_name)
        if let Some(inner) = rhs_str.strip_prefix("loadSelf(") {
            let block_name = inner.strip_suffix(')').unwrap_or(inner).to_string();
            bb.instructions.push(CfgInstruction::LoadSelf { lhs, block_name });
            return Ok(());
        }

        // load_yield_params(block_name)
        if let Some(inner) = rhs_str.strip_prefix("load_yield_params(") {
            let block_name = inner.strip_suffix(')').unwrap_or(inner).to_string();
            bb.instructions.push(CfgInstruction::LoadYieldParams { lhs, block_name });
            return Ok(());
        }

        // yield_load_arg(index, source)
        if let Some(inner) = rhs_str.strip_prefix("yield_load_arg(") {
            let inner = inner.strip_suffix(')').unwrap_or(inner);
            if let Some(comma_pos) = inner.find(", ") {
                let index: usize = inner[..comma_pos].parse().unwrap_or(0);
                let source = parse_typed_var_or_name(&inner[comma_pos + 2..]);
                bb.instructions.push(CfgInstruction::YieldLoadArg { lhs, index, source });
            }
            return Ok(());
        }

        // Solve<<temp>, block_name>
        if rhs_str.starts_with("Solve<") {
            let inner = rhs_str.strip_prefix("Solve<").unwrap();
            let inner = inner.strip_suffix('>').unwrap_or(inner);
            if let Some(comma_pos) = inner.rfind(", ") {
                let temp = inner[..comma_pos].to_string();
                let block_name = inner[comma_pos + 2..].to_string();
                bb.instructions.push(CfgInstruction::Solve { lhs, temp, block_name });
            }
            return Ok(());
        }

        // <keep-alive> var
        if let Some(var) = rhs_str.strip_prefix("<keep-alive> ") {
            bb.instructions.push(CfgInstruction::KeepAlive {
                lhs,
                var: var.to_string(),
            });
            return Ok(());
        }

        // Literal values: :symbol, "string", number, nil, true, false
        if rhs_str.starts_with(':') && !rhs_str.contains('.') {
            bb.instructions.push(CfgInstruction::Literal {
                lhs,
                value: rhs_str.to_string(),
            });
            return Ok(());
        }
        if rhs_str.starts_with('"') {
            bb.instructions.push(CfgInstruction::Literal {
                lhs,
                value: rhs_str.to_string(),
            });
            return Ok(());
        }
        if rhs_str == "nil" || rhs_str == "true" || rhs_str == "false" {
            bb.instructions.push(CfgInstruction::Literal {
                lhs,
                value: rhs_str.to_string(),
            });
            return Ok(());
        }
        if rhs_str.chars().next().is_some_and(|c| c.is_ascii_digit()) && !rhs_str.contains(':') {
            bb.instructions.push(CfgInstruction::Literal {
                lhs,
                value: rhs_str.to_string(),
            });
            return Ok(());
        }

        // Method call: <recv>: <RecvType>.<method>(<args>)
        // Check for receiver.method pattern
        if let Some(call) = try_parse_method_call(rhs_str) {
            bb.instructions.push(CfgInstruction::MethodCall {
                lhs,
                receiver: call.receiver,
                method_name: call.method_name,
                args: call.args,
            });
            return Ok(());
        }

        // Variable assignment (fallback): rhs is just a variable name/reference
        bb.instructions.push(CfgInstruction::Assignment {
            lhs,
            rhs: rhs_str.to_string(),
        });
    }

    Ok(())
}

/// Find " = " at top level (not inside brackets/parens/angles)
fn find_top_level_eq(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'<' | b'{' => depth += 1,
            b')' | b']' | b'>' | b'}' => depth -= 1,
            b' ' if depth == 0 => {
                if bytes.get(i + 1) == Some(&b'=') && bytes.get(i + 2) == Some(&b' ') {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Parse branch targets from "... ? bb<N> : bb<M>)"
fn parse_branch_targets(line: &str) -> Option<(usize, usize)> {
    let q_pos = line.rfind(" ? bb")?;
    let rest = &line[q_pos + 5..]; // after " ? bb" (5 chars)
    let colon_pos = rest.find(" : bb")?;
    let true_bb: usize = rest[..colon_pos].parse().ok()?;
    let after_colon = &rest[colon_pos + 5..]; // after ": bb"
    let end = after_colon.find(')').unwrap_or(after_colon.len());
    let false_bb: usize = after_colon[..end].parse().ok()?;
    Some((true_bb, false_bb))
}

/// Parse "name: Type" into CfgVar
fn parse_typed_var(s: &str) -> CfgVar {
    let s = s.trim();
    if let Some(colon_pos) = find_type_colon(s) {
        CfgVar {
            name: s[..colon_pos].to_string(),
            ty: s[colon_pos + 2..].to_string(),
        }
    } else {
        CfgVar {
            name: s.to_string(),
            ty: String::new(),
        }
    }
}

/// Parse a var that might or might not have a type annotation
fn parse_typed_var_or_name(s: &str) -> CfgVar {
    parse_typed_var(s)
}

/// Find the ": " that separates name from type, respecting nesting
fn find_type_colon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'<' | b'{' => depth += 1,
            b')' | b']' | b'>' | b'}' => depth -= 1,
            b':' if depth == 0 => {
                // Make sure it's ": " not "::" or ":symbol"
                if bytes.get(i + 1) == Some(&b' ') && i > 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Parse comma-separated typed vars, respecting nesting
fn parse_typed_var_list(s: &str) -> Vec<CfgVar> {
    let mut vars = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;

    for (i, b) in s.bytes().enumerate() {
        match b {
            b'(' | b'[' | b'<' | b'{' => depth += 1,
            b')' | b']' | b'>' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                let part = s[start..i].trim();
                if !part.is_empty() {
                    vars.push(parse_typed_var(part));
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let part = s[start..].trim();
    if !part.is_empty() {
        vars.push(parse_typed_var(part));
    }
    vars
}

struct ParsedCall {
    receiver: CfgVar,
    method_name: String,
    args: Vec<CfgVar>,
}

/// Try to parse "receiver: Type.method(args)" pattern
fn try_parse_method_call(s: &str) -> Option<ParsedCall> {
    // Find the method call dot: need to find ".<method>(" at top level after a type
    // Strategy: find the last ".method(" pattern where method is followed by "("
    let bytes = s.as_bytes();
    let mut depth = 0i32;

    // Find the opening paren of the argument list (the last top-level '(')
    let mut paren_pos = None;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    paren_pos = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let paren_pos = paren_pos?;

    // Now find the dot before the method name
    let before_paren = &s[..paren_pos];
    let dot_pos = find_method_dot(before_paren)?;

    let receiver_str = &s[..dot_pos];
    let method_name = s[dot_pos + 1..paren_pos].to_string();

    // Parse args inside parens
    let args_str = &s[paren_pos + 1..s.len().saturating_sub(1)]; // strip trailing )
    let args = if args_str.is_empty() {
        Vec::new()
    } else {
        parse_typed_var_list(args_str)
    };

    let receiver = parse_typed_var(receiver_str);

    Some(ParsedCall {
        receiver,
        method_name,
        args,
    })
}

/// Find the dot that separates receiver from method name.
/// This is the last '.' at top level that is followed by a valid method identifier.
fn find_method_dot(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut last_dot = None;

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' | b'[' | b'<' | b'{' => depth += 1,
            b')' | b']' | b'>' | b'}' => depth -= 1,
            b'.' if depth == 0 => {
                // Check it's not ".." and followed by a method-name char
                if bytes.get(i + 1).is_some_and(|&c| c != b'.') {
                    last_dot = Some(i);
                }
            }
            _ => {}
        }
    }

    last_dot
}

/// Find the last comma at top level for splitting cast args
fn find_last_comma_top_level(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut last_comma = None;

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' | b'[' | b'<' | b'{' => depth += 1,
            b')' | b']' | b'>' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                last_comma = Some(i);
            }
            _ => {}
        }
    }

    last_comma
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_method() {
        let input = r#"method ::AdminArea::CampaignsController#edit {

bb0[firstDead=5]():
    <self>: AdminArea::CampaignsController = cast(<self>: NilClass, AdminArea::CampaignsController);
    <hashTemp>$4: Symbol(:layout) = :layout
    <hashTemp>$5: String("admin_area/project_editor") = "admin_area/project_editor"
    <returnMethodTemp>$2: T.untyped = <self>: AdminArea::CampaignsController.render(<hashTemp>$4: Symbol(:layout), <hashTemp>$5: String("admin_area/project_editor"))
    <finalReturn>: T.noreturn = return <returnMethodTemp>$2: T.untyped
    <unconditional> -> bb1

# backedges
# - bb0
bb1[firstDead=-1]():
    <unconditional> -> bb1

}"#;

        let methods = parse(input).unwrap();
        assert_eq!(methods.len(), 1);
        let m = &methods[0];
        assert_eq!(m.raw_name, "::AdminArea::CampaignsController#edit");
        assert_eq!(m.blocks.len(), 2);

        let bb0 = &m.blocks[0];
        assert_eq!(bb0.id, 0);
        assert_eq!(bb0.first_dead, 5);
        assert_eq!(bb0.instructions.len(), 5);

        // cast
        assert!(matches!(&bb0.instructions[0], CfgInstruction::Cast { .. }));
        // literal :layout
        assert!(matches!(&bb0.instructions[1], CfgInstruction::Literal { value, .. } if value == ":layout"));
        // literal string
        assert!(matches!(&bb0.instructions[2], CfgInstruction::Literal { value, .. } if value == "\"admin_area/project_editor\""));
        // method call render
        assert!(matches!(&bb0.instructions[3], CfgInstruction::MethodCall { method_name, .. } if method_name == "render"));
        // return
        assert!(matches!(&bb0.instructions[4], CfgInstruction::Return { .. }));

        let bb1 = &m.blocks[1];
        assert_eq!(bb1.id, 1);
        assert_eq!(bb1.backedges, vec![0]);
    }

    #[test]
    fn test_parse_conditional_branch() {
        let input = r#"method ::Foo#bar {

bb0[firstDead=-1]():
    <ifTemp>$3: T.untyped = <self>: Foo.check()
    <ifTemp>$3 -> (T.untyped ? bb2 : bb3)

bb1[firstDead=-1]():
    <unconditional> -> bb1

bb2[firstDead=-1]():
    <unconditional> -> bb1

bb3[firstDead=-1]():
    <unconditional> -> bb1

}"#;

        let methods = parse(input).unwrap();
        let bb0 = &methods[0].blocks[0];
        assert!(matches!(
            &bb0.terminator,
            CfgTerminator::Conditional { true_bb: 2, false_bb: 3, .. }
        ));
    }

    #[test]
    fn test_parse_alias_ivar() {
        let input = r#"method ::Foo#bar {

bb0[firstDead=-1]():
    @campaigns$4: T.untyped = alias <C <undeclared-field-stub>> (@campaigns)
    @tree$4: T.untyped = alias @tree
    <cfgAlias>$7: T.class_of(T) = alias <C T>
    <unconditional> -> bb1

bb1[firstDead=-1]():
    <unconditional> -> bb1

}"#;

        let methods = parse(input).unwrap();
        let bb0 = &methods[0].blocks[0];
        assert!(matches!(&bb0.instructions[0], CfgInstruction::Alias { kind: AliasKind::Ivar(name), .. } if name == "@campaigns"));
        assert!(matches!(&bb0.instructions[1], CfgInstruction::Alias { kind: AliasKind::Ivar(name), .. } if name == "@tree"));
        assert!(matches!(&bb0.instructions[2], CfgInstruction::Alias { kind: AliasKind::Constant(name), .. } if name == "T"));
    }

    #[test]
    fn test_parse_load_arg() {
        let input = r#"method ::Foo#bar {

bb0[firstDead=-1]():
    slot_id: String = load_arg(slot_id)
    <argPresent>$3: T::Boolean = arg_present(at)
    <unconditional> -> bb1

bb1[firstDead=-1]():
    <unconditional> -> bb1

}"#;

        let methods = parse(input).unwrap();
        let bb0 = &methods[0].blocks[0];
        assert!(matches!(&bb0.instructions[0], CfgInstruction::LoadArg { arg_name, .. } if arg_name == "slot_id"));
        assert!(matches!(&bb0.instructions[1], CfgInstruction::ArgPresent { arg_name, .. } if arg_name == "at"));
    }

    #[test]
    fn test_parse_block_call() {
        let input = r#"method ::Foo#bar {

bb0[firstDead=-1]():
    <unconditional> -> bb2

bb1[firstDead=-1]():
    <unconditional> -> bb1

bb2[firstDead=-1]():
    # outerLoops: 1
    <block-call> -> (NilClass ? bb5 : bb3)

bb3[firstDead=-1]():
    <unconditional> -> bb1

bb5[firstDead=-1]():
    <self>: Foo = loadSelf(find)
    <blk>$8: [String, Integer] = load_yield_params(find)
    _$1: String = yield_load_arg(0, <blk>$8: [String, Integer])
    <blockReturnTemp>$9: T::Boolean = <self>: Foo.check()
    <blockReturnTemp>$16: T.noreturn = blockreturn<find> <blockReturnTemp>$9: T::Boolean
    <unconditional> -> bb2

}"#;

        let methods = parse(input).unwrap();
        let bb2 = &methods[0].blocks[2];
        assert!(matches!(&bb2.terminator, CfgTerminator::BlockCall { true_bb: 5, false_bb: 3 }));

        let bb5 = &methods[0].blocks[4]; // bb0, bb1, bb2, bb3, bb5
        assert!(matches!(&bb5.instructions[0], CfgInstruction::LoadSelf { block_name, .. } if block_name == "find"));
        assert!(matches!(&bb5.instructions[1], CfgInstruction::LoadYieldParams { block_name, .. } if block_name == "find"));
        assert!(matches!(&bb5.instructions[2], CfgInstruction::YieldLoadArg { index: 0, .. }));
        assert!(matches!(&bb5.instructions[4], CfgInstruction::BlockReturn { block_name, .. } if block_name == "find"));
    }

    #[test]
    fn test_parse_exception_handling() {
        let input = r#"method ::Foo#bar {

bb0[firstDead=-1]():
    <exceptionValue>$3: T.nilable(Exception) = <get-current-exception>
    <exceptionValue>$3 -> (T.nilable(Exception) ? bb3 : bb4)

bb1[firstDead=-1]():
    <unconditional> -> bb1

bb3[firstDead=-1](<exceptionValue>$3: Exception):
    <cfgAlias>$13: T.class_of(ArgumentError) = alias <C ArgumentError>
    <isaCheckTemp>$14: T::Boolean = <cfgAlias>$13: T.class_of(ArgumentError).===(<exceptionValue>$3: Exception)
    <isaCheckTemp>$14 -> (T::Boolean ? bb7 : bb8)

bb4[firstDead=-1]():
    <unconditional> -> bb1

bb7[firstDead=-1]():
    <unconditional> -> bb1

bb8[firstDead=-1]():
    <unconditional> -> bb1

}"#;

        let methods = parse(input).unwrap();
        let bb0 = &methods[0].blocks[0];
        assert!(matches!(&bb0.instructions[0], CfgInstruction::GetCurrentException { .. }));

        let bb3 = &methods[0].blocks[2];
        assert_eq!(bb3.params.len(), 1);
        assert_eq!(bb3.params[0].name, "<exceptionValue>$3");
    }

    #[test]
    fn test_parse_multiple_methods() {
        let input = r#"method ::Foo#a {

bb0[firstDead=-1]():
    <unconditional> -> bb1

bb1[firstDead=-1]():
    <unconditional> -> bb1

}

method ::Foo#b {

bb0[firstDead=-1]():
    <unconditional> -> bb1

bb1[firstDead=-1]():
    <unconditional> -> bb1

}"#;

        let methods = parse(input).unwrap();
        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].raw_name, "::Foo#a");
        assert_eq!(methods[1].raw_name, "::Foo#b");
    }
}
