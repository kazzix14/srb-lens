use magnus::{
    Error, Ruby, RArray, RHash, Value,
    function, method,
    prelude::*,
    scan_args::scan_args,
};
use srb_lens::indexer::{self, SrbCommand};
use srb_lens::model::{self, Terminator};
use std::path::Path;

fn ruby() -> Ruby {
    Ruby::get().unwrap()
}

fn err(msg: String) -> Error {
    Error::new(ruby().exception_runtime_error(), msg)
}

// ─── Project ────────────────────────────────────────────────────────

#[magnus::wrap(class = "SrbLens::Project")]
struct RbProject {
    inner: model::Project,
}

impl RbProject {
    fn load_from_cache(dir: String) -> Result<Self, Error> {
        let project = indexer::load_from_cache(Path::new(&dir)).map_err(|e| err(e.to_string()))?;
        Ok(Self { inner: project })
    }

    fn load_or_index(args: &[Value]) -> Result<Self, Error> {
        let parsed = scan_args::<(String,), (), (), (), RHash, ()>(args)?;
        let dir = parsed.required.0;
        let srb_command = if parsed.keywords.is_nil() {
            SrbCommand::default()
        } else {
            let val: Option<String> =
                parsed.keywords.aref(ruby().to_symbol("srb_command"))?;
            match val {
                Some(cmd) => SrbCommand::new(&cmd),
                None => SrbCommand::default(),
            }
        };

        let project =
            indexer::load_or_index(Path::new(&dir), &srb_command).map_err(|e| err(e.to_string()))?;
        Ok(Self { inner: project })
    }

    fn index(args: &[Value]) -> Result<Self, Error> {
        let parsed = scan_args::<(String,), (), (), (), RHash, ()>(args)?;
        let dir = parsed.required.0;
        let srb_command = if parsed.keywords.is_nil() {
            SrbCommand::default()
        } else {
            let val: Option<String> =
                parsed.keywords.aref(ruby().to_symbol("srb_command"))?;
            match val {
                Some(cmd) => SrbCommand::new(&cmd),
                None => SrbCommand::default(),
            }
        };

        let project =
            indexer::index(Path::new(&dir), &srb_command).map_err(|e| err(e.to_string()))?;
        Ok(Self { inner: project })
    }

    fn find_methods(&self, query: String) -> RArray {
        let ruby = ruby();
        let results = self.inner.find_methods(&query);
        let ary = ruby.ary_new_capa(results.len());
        for m in results {
            let _ = ary.push(RbMethodInfo::from_model(m));
        }
        ary
    }

    fn find_classes(&self, query: String) -> RArray {
        let ruby = ruby();
        let results = self.inner.find_classes(&query);
        let ary = ruby.ary_new_capa(results.len());
        for c in results {
            let _ = ary.push(RbClassInfo::from_model(c));
        }
        ary
    }
}

// ─── MethodInfo ─────────────────────────────────────────────────────

#[magnus::wrap(class = "SrbLens::MethodInfo")]
struct RbMethodInfo {
    fqn: String,
    file_path: Option<String>,
    line: Option<usize>,
    return_type: Option<String>,
    arguments: Vec<model::Argument>,
    calls: Vec<model::MethodCall>,
    ivars: Vec<model::IvarAccess>,
    rescues: Vec<String>,
    uses_block: bool,
    basic_blocks: Vec<RbBasicBlockData>,
}

struct RbBasicBlockData {
    id: usize,
    terminator: String,
}

impl RbMethodInfo {
    fn from_model(m: &model::MethodInfo) -> Self {
        let basic_blocks = m
            .basic_blocks
            .iter()
            .map(|bb| {
                let terminator = match &bb.terminator {
                    Terminator::Goto(target) => format!("goto bb{target}"),
                    Terminator::Branch {
                        condition,
                        true_bb,
                        false_bb,
                    } => format!("branch {condition} ? bb{true_bb} : bb{false_bb}"),
                    Terminator::BlockCall { true_bb, false_bb } => {
                        format!("block_call bb{true_bb} / bb{false_bb}")
                    }
                    Terminator::Return => "return".to_string(),
                };
                RbBasicBlockData {
                    id: bb.id,
                    terminator,
                }
            })
            .collect();

        Self {
            fqn: m.fqn.to_string(),
            file_path: m.file_path.clone(),
            line: m.line,
            return_type: m.return_type.as_ref().map(|t| t.to_string()),
            arguments: m.arguments.clone(),
            calls: m.calls.clone(),
            ivars: m.ivars.clone(),
            rescues: m.rescues.clone(),
            uses_block: m.uses_block,
            basic_blocks,
        }
    }

    fn fqn(&self) -> &str {
        &self.fqn
    }

    fn file_path(&self) -> Option<&str> {
        self.file_path.as_deref()
    }

    fn line(&self) -> Option<usize> {
        self.line
    }

    fn return_type(&self) -> Option<&str> {
        self.return_type.as_deref()
    }

    fn uses_block(&self) -> bool {
        self.uses_block
    }

    fn arguments(&self) -> RArray {
        let ruby = ruby();
        let ary = ruby.ary_new_capa(self.arguments.len());
        for a in &self.arguments {
            let _ = ary.push(RbArgument::from_model(a));
        }
        ary
    }

    fn calls(&self) -> RArray {
        let ruby = ruby();
        let ary = ruby.ary_new_capa(self.calls.len());
        for c in &self.calls {
            let _ = ary.push(RbMethodCall::from_model(c));
        }
        ary
    }

    fn ivars(&self) -> RArray {
        let ruby = ruby();
        let ary = ruby.ary_new_capa(self.ivars.len());
        for iv in &self.ivars {
            let _ = ary.push(RbIvarAccess::from_model(iv));
        }
        ary
    }

    fn rescues(&self) -> RArray {
        let ruby = ruby();
        let ary = ruby.ary_new_capa(self.rescues.len());
        for r in &self.rescues {
            let _ = ary.push(r.as_str());
        }
        ary
    }

    fn basic_blocks(&self) -> RArray {
        let ruby = ruby();
        let ary = ruby.ary_new_capa(self.basic_blocks.len());
        for bb in &self.basic_blocks {
            let _ = ary.push(RbBasicBlock {
                id: bb.id,
                terminator: bb.terminator.clone(),
            });
        }
        ary
    }
}

// ─── ClassInfo ──────────────────────────────────────────────────────

#[magnus::wrap(class = "SrbLens::ClassInfo")]
struct RbClassInfo {
    fqn: String,
    is_module: bool,
    super_class: Option<String>,
    mixins: Vec<String>,
    file_path: Option<String>,
    line: Option<usize>,
}

impl RbClassInfo {
    fn from_model(c: &model::ClassInfo) -> Self {
        Self {
            fqn: c.fqn.clone(),
            is_module: c.is_module,
            super_class: c.super_class.clone(),
            mixins: c.mixins.clone(),
            file_path: c.file_path.clone(),
            line: c.line,
        }
    }

    fn fqn(&self) -> &str {
        &self.fqn
    }

    fn is_module(&self) -> bool {
        self.is_module
    }

    fn super_class(&self) -> Option<&str> {
        self.super_class.as_deref()
    }

    fn mixins(&self) -> RArray {
        let ruby = ruby();
        let ary = ruby.ary_new_capa(self.mixins.len());
        for m in &self.mixins {
            let _ = ary.push(m.as_str());
        }
        ary
    }

    fn file_path(&self) -> Option<&str> {
        self.file_path.as_deref()
    }

    fn line(&self) -> Option<usize> {
        self.line
    }
}

// ─── Argument ───────────────────────────────────────────────────────

#[magnus::wrap(class = "SrbLens::Argument")]
struct RbArgument {
    name: String,
    ty: String,
    kind: String,
}

impl RbArgument {
    fn from_model(a: &model::Argument) -> Self {
        let kind = match a.kind {
            model::ArgumentKind::Req => "req",
            model::ArgumentKind::Opt => "opt",
            model::ArgumentKind::Rest => "rest",
            model::ArgumentKind::KeyReq => "keyreq",
            model::ArgumentKind::Key => "key",
            model::ArgumentKind::KeyRest => "keyrest",
            model::ArgumentKind::Block => "block",
        };
        Self {
            name: a.name.clone(),
            ty: a.ty.to_string(),
            kind: kind.to_string(),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn ty(&self) -> &str {
        &self.ty
    }

    fn kind(&self) -> &str {
        &self.kind
    }
}

// ─── MethodCall ─────────────────────────────────────────────────────

#[magnus::wrap(class = "SrbLens::MethodCall")]
struct RbMethodCall {
    receiver_type: String,
    method_name: String,
    return_type: String,
    conditions: Vec<model::BranchCondition>,
}

impl RbMethodCall {
    fn from_model(c: &model::MethodCall) -> Self {
        Self {
            receiver_type: c.receiver_type.to_string(),
            method_name: c.method_name.clone(),
            return_type: c.return_type.to_string(),
            conditions: c.conditions.clone(),
        }
    }

    fn receiver_type(&self) -> &str {
        &self.receiver_type
    }

    fn method_name(&self) -> &str {
        &self.method_name
    }

    fn return_type(&self) -> &str {
        &self.return_type
    }

    fn conditions(&self) -> RArray {
        let ruby = ruby();
        let ary = ruby.ary_new_capa(self.conditions.len());
        for cond in &self.conditions {
            let _ = ary.push(RbBranchCondition::from_model(cond));
        }
        ary
    }
}

// ─── BranchCondition ────────────────────────────────────────────────

#[magnus::wrap(class = "SrbLens::BranchCondition")]
struct RbBranchCondition {
    call: String,
    is_true: bool,
}

impl RbBranchCondition {
    fn from_model(c: &model::BranchCondition) -> Self {
        Self {
            call: c.call.clone(),
            is_true: c.is_true,
        }
    }

    fn call(&self) -> &str {
        &self.call
    }

    fn is_true(&self) -> bool {
        self.is_true
    }
}

// ─── IvarAccess ─────────────────────────────────────────────────────

#[magnus::wrap(class = "SrbLens::IvarAccess")]
struct RbIvarAccess {
    name: String,
    ty: String,
}

impl RbIvarAccess {
    fn from_model(iv: &model::IvarAccess) -> Self {
        Self {
            name: iv.name.clone(),
            ty: iv.ty.to_string(),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn ty(&self) -> &str {
        &self.ty
    }
}

// ─── BasicBlock ─────────────────────────────────────────────────────

#[magnus::wrap(class = "SrbLens::BasicBlock")]
struct RbBasicBlock {
    id: usize,
    terminator: String,
}

impl RbBasicBlock {
    fn id(&self) -> usize {
        self.id
    }

    fn terminator(&self) -> &str {
        &self.terminator
    }
}

// ─── Init ───────────────────────────────────────────────────────────

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("SrbLens")?;

    // Project
    let project = module.define_class("Project", ruby.class_object())?;
    project.define_singleton_method("load_from_cache", function!(RbProject::load_from_cache, 1))?;
    project.define_singleton_method("load_or_index", function!(RbProject::load_or_index, -1))?;
    project.define_singleton_method("index", function!(RbProject::index, -1))?;
    project.define_method("find_methods", method!(RbProject::find_methods, 1))?;
    project.define_method("find_classes", method!(RbProject::find_classes, 1))?;

    // MethodInfo
    let method_info = module.define_class("MethodInfo", ruby.class_object())?;
    method_info.define_method("fqn", method!(RbMethodInfo::fqn, 0))?;
    method_info.define_method("file_path", method!(RbMethodInfo::file_path, 0))?;
    method_info.define_method("line", method!(RbMethodInfo::line, 0))?;
    method_info.define_method("return_type", method!(RbMethodInfo::return_type, 0))?;
    method_info.define_method("uses_block", method!(RbMethodInfo::uses_block, 0))?;
    method_info.define_method("arguments", method!(RbMethodInfo::arguments, 0))?;
    method_info.define_method("calls", method!(RbMethodInfo::calls, 0))?;
    method_info.define_method("ivars", method!(RbMethodInfo::ivars, 0))?;
    method_info.define_method("rescues", method!(RbMethodInfo::rescues, 0))?;
    method_info.define_method("basic_blocks", method!(RbMethodInfo::basic_blocks, 0))?;

    // ClassInfo
    let class_info = module.define_class("ClassInfo", ruby.class_object())?;
    class_info.define_method("fqn", method!(RbClassInfo::fqn, 0))?;
    class_info.define_method("is_module", method!(RbClassInfo::is_module, 0))?;
    class_info.define_method("super_class", method!(RbClassInfo::super_class, 0))?;
    class_info.define_method("mixins", method!(RbClassInfo::mixins, 0))?;
    class_info.define_method("file_path", method!(RbClassInfo::file_path, 0))?;
    class_info.define_method("line", method!(RbClassInfo::line, 0))?;

    // Argument
    let argument = module.define_class("Argument", ruby.class_object())?;
    argument.define_method("name", method!(RbArgument::name, 0))?;
    argument.define_method("type", method!(RbArgument::ty, 0))?;
    argument.define_method("kind", method!(RbArgument::kind, 0))?;

    // MethodCall
    let method_call = module.define_class("MethodCall", ruby.class_object())?;
    method_call.define_method("receiver_type", method!(RbMethodCall::receiver_type, 0))?;
    method_call.define_method("method_name", method!(RbMethodCall::method_name, 0))?;
    method_call.define_method("return_type", method!(RbMethodCall::return_type, 0))?;
    method_call.define_method("conditions", method!(RbMethodCall::conditions, 0))?;

    // BranchCondition
    let branch_cond = module.define_class("BranchCondition", ruby.class_object())?;
    branch_cond.define_method("call", method!(RbBranchCondition::call, 0))?;
    branch_cond.define_method("true?", method!(RbBranchCondition::is_true, 0))?;

    // IvarAccess
    let ivar_access = module.define_class("IvarAccess", ruby.class_object())?;
    ivar_access.define_method("name", method!(RbIvarAccess::name, 0))?;
    ivar_access.define_method("type", method!(RbIvarAccess::ty, 0))?;

    // BasicBlock
    let basic_block = module.define_class("BasicBlock", ruby.class_object())?;
    basic_block.define_method("id", method!(RbBasicBlock::id, 0))?;
    basic_block.define_method("terminator", method!(RbBasicBlock::terminator, 0))?;

    Ok(())
}
