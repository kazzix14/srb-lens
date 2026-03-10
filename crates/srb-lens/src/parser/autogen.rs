#[derive(Debug)]
pub struct AutogenFile {
    pub path: String,
    pub requires: Vec<String>,
    pub defs: Vec<AutogenDef>,
    pub refs: Vec<AutogenRef>,
}

#[derive(Debug)]
pub struct AutogenDef {
    pub id: usize,
    pub kind: DefKind,
    pub defines_behavior: bool,
    pub is_empty: bool,
    pub defining_ref: Option<Vec<String>>,
    pub parent_ref: Option<Vec<String>>,
    pub aliased_ref: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    Class,
    Module,
}

#[derive(Debug)]
pub struct AutogenRef {
    pub id: usize,
    pub scope: Vec<String>,
    pub name: Vec<String>,
    pub nesting: Vec<Vec<String>>,
    pub resolved: Vec<String>,
    pub loc: String,
    pub is_defining_ref: bool,
    pub parent_of: Option<Vec<String>>,
}

#[derive(Debug, thiserror::Error)]
pub enum AutogenParseError {
    #[error("parse error at line {line}: {message}")]
    ParseError { line: usize, message: String },
}

pub fn parse(input: &str) -> Result<Vec<AutogenFile>, AutogenParseError> {
    let mut files = Vec::new();
    let mut current_file: Option<AutogenFileBuilder> = None;
    let mut section = Section::None;
    let mut current_def: Option<AutogenDefBuilder> = None;
    let mut current_ref: Option<AutogenRefBuilder> = None;

    for (_line_no, line) in input.lines().enumerate() {
        // New file section
        if let Some(path) = line.strip_prefix("# ParsedFile: ") {
            // Flush previous
            flush_entry(&mut current_def, &mut current_ref, &mut current_file);
            if let Some(file) = current_file.take() {
                files.push(file.finish());
            }
            current_file = Some(AutogenFileBuilder {
                path: path.to_string(),
                requires: Vec::new(),
                defs: Vec::new(),
                refs: Vec::new(),
            });
            section = Section::None;
            continue;
        }

        // requires: [...]
        if let Some(rest) = line.strip_prefix("requires: ") {
            if let Some(file) = &mut current_file {
                file.requires = parse_bracket_list(rest);
            }
            continue;
        }

        // Section headers
        if line == "## defs:" {
            flush_entry(&mut current_def, &mut current_ref, &mut current_file);
            section = Section::Defs;
            continue;
        }
        if line == "## refs:" {
            flush_entry(&mut current_def, &mut current_ref, &mut current_file);
            section = Section::Refs;
            continue;
        }

        // Entry headers
        if let Some(rest) = line.strip_prefix("[def id=") {
            flush_entry(&mut current_def, &mut current_ref, &mut current_file);
            let id_str = rest.strip_suffix(']').unwrap_or(rest);
            let id: usize = id_str.parse().unwrap_or(0);
            current_def = Some(AutogenDefBuilder::new(id));
            continue;
        }
        if let Some(rest) = line.strip_prefix("[ref id=") {
            flush_entry(&mut current_def, &mut current_ref, &mut current_file);
            let id_str = rest.strip_suffix(']').unwrap_or(rest);
            let id: usize = id_str.parse().unwrap_or(0);
            current_ref = Some(AutogenRefBuilder::new(id));
            continue;
        }

        // Field lines (space-prefixed)
        if let Some(field_line) = line.strip_prefix(' ') {
            match section {
                Section::Defs => {
                    if let Some(def) = &mut current_def {
                        parse_def_field(def, field_line);
                    }
                }
                Section::Refs => {
                    if let Some(refb) = &mut current_ref {
                        parse_ref_field(refb, field_line);
                    }
                }
                Section::None => {}
            }
        }
    }

    // Flush remaining
    flush_entry(&mut current_def, &mut current_ref, &mut current_file);
    if let Some(file) = current_file.take() {
        files.push(file.finish());
    }

    Ok(files)
}

#[derive(Clone, Copy)]
enum Section {
    None,
    Defs,
    Refs,
}

struct AutogenFileBuilder {
    path: String,
    requires: Vec<String>,
    defs: Vec<AutogenDef>,
    refs: Vec<AutogenRef>,
}

impl AutogenFileBuilder {
    fn finish(self) -> AutogenFile {
        AutogenFile {
            path: self.path,
            requires: self.requires,
            defs: self.defs,
            refs: self.refs,
        }
    }
}

struct AutogenDefBuilder {
    id: usize,
    kind: Option<DefKind>,
    defines_behavior: bool,
    is_empty: bool,
    defining_ref: Option<Vec<String>>,
    parent_ref: Option<Vec<String>>,
    aliased_ref: Option<Vec<String>>,
}

impl AutogenDefBuilder {
    fn new(id: usize) -> Self {
        Self {
            id,
            kind: None,
            defines_behavior: false,
            is_empty: false,
            defining_ref: None,
            parent_ref: None,
            aliased_ref: None,
        }
    }

    fn finish(self) -> AutogenDef {
        AutogenDef {
            id: self.id,
            kind: self.kind.unwrap_or(DefKind::Class),
            defines_behavior: self.defines_behavior,
            is_empty: self.is_empty,
            defining_ref: self.defining_ref,
            parent_ref: self.parent_ref,
            aliased_ref: self.aliased_ref,
        }
    }
}

struct AutogenRefBuilder {
    id: usize,
    scope: Vec<String>,
    name: Vec<String>,
    nesting: Vec<Vec<String>>,
    resolved: Vec<String>,
    loc: String,
    is_defining_ref: bool,
    parent_of: Option<Vec<String>>,
}

impl AutogenRefBuilder {
    fn new(id: usize) -> Self {
        Self {
            id,
            scope: Vec::new(),
            name: Vec::new(),
            nesting: Vec::new(),
            resolved: Vec::new(),
            loc: String::new(),
            is_defining_ref: false,
            parent_of: None,
        }
    }

    fn finish(self) -> AutogenRef {
        AutogenRef {
            id: self.id,
            scope: self.scope,
            name: self.name,
            nesting: self.nesting,
            resolved: self.resolved,
            loc: self.loc,
            is_defining_ref: self.is_defining_ref,
            parent_of: self.parent_of,
        }
    }
}

fn flush_entry(
    current_def: &mut Option<AutogenDefBuilder>,
    current_ref: &mut Option<AutogenRefBuilder>,
    current_file: &mut Option<AutogenFileBuilder>,
) {
    if let Some(def) = current_def.take() {
        if let Some(file) = current_file {
            file.defs.push(def.finish());
        }
    }
    if let Some(refb) = current_ref.take() {
        if let Some(file) = current_file {
            file.refs.push(refb.finish());
        }
    }
}

fn parse_def_field(def: &mut AutogenDefBuilder, field: &str) {
    if let Some(val) = field.strip_prefix("type=") {
        def.kind = Some(match val {
            "class" => DefKind::Class,
            "module" => DefKind::Module,
            _ => DefKind::Class,
        });
    } else if let Some(val) = field.strip_prefix("defines_behavior=") {
        def.defines_behavior = val == "1";
    } else if let Some(val) = field.strip_prefix("is_empty=") {
        def.is_empty = val == "1";
    } else if let Some(val) = field.strip_prefix("defining_ref=") {
        def.defining_ref = Some(parse_bracket_names(val));
    } else if let Some(val) = field.strip_prefix("parent_ref=") {
        def.parent_ref = Some(parse_bracket_names(val));
    } else if let Some(val) = field.strip_prefix("aliased_ref=") {
        def.aliased_ref = Some(parse_bracket_names(val));
    }
}

fn parse_ref_field(refb: &mut AutogenRefBuilder, field: &str) {
    if let Some(val) = field.strip_prefix("scope=") {
        refb.scope = parse_bracket_names(val);
    } else if let Some(val) = field.strip_prefix("name=") {
        refb.name = parse_bracket_names(val);
    } else if let Some(val) = field.strip_prefix("nesting=") {
        refb.nesting = parse_nesting(val);
    } else if let Some(val) = field.strip_prefix("resolved=") {
        refb.resolved = parse_bracket_names(val);
    } else if let Some(val) = field.strip_prefix("loc=") {
        refb.loc = val.to_string();
    } else if let Some(val) = field.strip_prefix("is_defining_ref=") {
        refb.is_defining_ref = val == "1";
    } else if let Some(val) = field.strip_prefix("parent_of=") {
        refb.parent_of = Some(parse_bracket_names(val));
    }
}

/// Parse "[Name1 Name2]" → ["Name1", "Name2"]
fn parse_bracket_names(s: &str) -> Vec<String> {
    let s = s.trim();
    let inner = s.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(s);
    if inner.is_empty() {
        return Vec::new();
    }
    inner.split_whitespace().map(String::from).collect()
}

/// Parse "[[A B] [C]]" → [["A", "B"], ["C"]]
fn parse_nesting(s: &str) -> Vec<Vec<String>> {
    let s = s.trim();
    let inner = s.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(s);
    if inner.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut depth = 0;
    let mut start = 0;

    for (i, c) in inner.char_indices() {
        match c {
            '[' => {
                if depth == 0 {
                    start = i + 1;
                }
                depth += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    let group = &inner[start..i];
                    result.push(group.split_whitespace().map(String::from).collect());
                }
            }
            _ => {}
        }
    }

    result
}

/// Parse bracket list like "[foo, bar]" or "[]"
fn parse_bracket_list(s: &str) -> Vec<String> {
    let s = s.trim();
    let inner = s.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(s);
    if inner.is_empty() {
        return Vec::new();
    }
    inner.split(',').map(|s| s.trim().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sample_file() {
        let input = r#"# ParsedFile: ./app/components/datetime_picker_component.rb
requires: []
## defs:
[def id=0]
 type=module
 defines_behavior=0
 is_empty=0
[def id=1]
 type=class
 defines_behavior=1
 is_empty=0
 defining_ref=[DatetimePickerComponent]
 parent_ref=[ApplicationComponent]
## refs:
[ref id=0]
 scope=[]
 name=[DatetimePickerComponent]
 nesting=[]
 resolved=[DatetimePickerComponent]
 loc=app/components/datetime_picker_component.rb:4
 is_defining_ref=1
[ref id=1]
 scope=[]
 name=[ApplicationComponent]
 nesting=[]
 resolved=[ApplicationComponent]
 loc=app/components/datetime_picker_component.rb:4
 is_defining_ref=0
 parent_of=[DatetimePickerComponent]
[ref id=2]
 scope=[DatetimePickerComponent]
 name=[SecureRandom]
 nesting=[[DatetimePickerComponent]]
 resolved=[SecureRandom]
 loc=app/components/datetime_picker_component.rb:9
 is_defining_ref=0"#;

        let files = parse(input).unwrap();
        assert_eq!(files.len(), 1);

        let f = &files[0];
        assert_eq!(f.path, "./app/components/datetime_picker_component.rb");
        assert!(f.requires.is_empty());

        // defs
        assert_eq!(f.defs.len(), 2);
        assert_eq!(f.defs[0].kind, DefKind::Module);
        assert!(!f.defs[0].defines_behavior);
        assert_eq!(f.defs[1].kind, DefKind::Class);
        assert!(f.defs[1].defines_behavior);
        assert_eq!(
            f.defs[1].defining_ref.as_deref(),
            Some(["DatetimePickerComponent".to_string()].as_slice())
        );
        assert_eq!(
            f.defs[1].parent_ref.as_deref(),
            Some(["ApplicationComponent".to_string()].as_slice())
        );

        // refs
        assert_eq!(f.refs.len(), 3);

        let r0 = &f.refs[0];
        assert_eq!(r0.resolved, vec!["DatetimePickerComponent"]);
        assert!(r0.is_defining_ref);

        let r1 = &f.refs[1];
        assert_eq!(r1.resolved, vec!["ApplicationComponent"]);
        assert!(!r1.is_defining_ref);
        assert_eq!(
            r1.parent_of.as_deref(),
            Some(["DatetimePickerComponent".to_string()].as_slice())
        );

        let r2 = &f.refs[2];
        assert_eq!(r2.scope, vec!["DatetimePickerComponent"]);
        assert_eq!(r2.name, vec!["SecureRandom"]);
        assert_eq!(r2.nesting, vec![vec!["DatetimePickerComponent".to_string()]]);
        assert_eq!(r2.loc, "app/components/datetime_picker_component.rb:9");
    }

    #[test]
    fn test_parse_nested_names() {
        let input = r#"# ParsedFile: ./app/controllers/user_area/campaigns_controller.rb
requires: []
## defs:
[def id=0]
 type=module
 defines_behavior=0
 is_empty=0
## refs:
[ref id=0]
 scope=[]
 name=[UserArea Campaigns Sections ProjectForwardingComponent]
 nesting=[]
 resolved=[UserArea Campaigns Sections ProjectForwardingComponent]
 loc=app/controllers/user_area/campaigns_controller.rb:10
 is_defining_ref=0"#;

        let files = parse(input).unwrap();
        let r = &files[0].refs[0];
        assert_eq!(
            r.resolved,
            vec!["UserArea", "Campaigns", "Sections", "ProjectForwardingComponent"]
        );
    }

    #[test]
    fn test_parse_multiple_files() {
        let input = r#"# ParsedFile: ./a.rb
requires: []
## defs:
## refs:
# ParsedFile: ./b.rb
requires: []
## defs:
## refs:"#;

        let files = parse(input).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "./a.rb");
        assert_eq!(files[1].path, "./b.rb");
    }
}
