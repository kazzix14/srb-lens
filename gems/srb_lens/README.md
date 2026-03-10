# srb_lens

Ruby bindings for [srb-lens](../../), a static analysis tool for [Sorbet](https://sorbet.org/)-typed Ruby projects. Powered by a Rust native extension via [Magnus](https://github.com/matsadler/magnus).

srb-lens parses Sorbet's CFG output, symbol tables, and autogen data to build a queryable model of your project's methods, classes, types, and control flow.

## Requirements

- Ruby >= 3.1
- Rust toolchain (stable)
- Sorbet (`srb`) available in the target project

## Installation

From the repository root:

```bash
cd gems/srb_lens
bundle install
bundle exec rake compile
```

## Usage

### Loading a project

```ruby
require "srb_lens"

# Load from existing cache (.srb-lens/ directory)
project = SrbLens::Project.load_from_cache("/path/to/project")

# Load from cache if available, otherwise run Sorbet to index
project = SrbLens::Project.load_or_index("/path/to/project")

# With a custom srb command
project = SrbLens::Project.load_or_index("/path/to/project", srb_command: "bundle exec srb")

# Force a fresh index (always runs Sorbet)
project = SrbLens::Project.index("/path/to/project")
```

### Querying methods

```ruby
# Instance method: "ClassName#method_name"
methods = project.find_methods("Campaign#active?")

# Class method: "ClassName.method_name"
methods = project.find_methods("User.find_by_email")

# All methods in a class
methods = project.find_methods("Campaign")

methods.each do |m|
  m.fqn           # => "Campaign#active?"
  m.file_path     # => "app/models/campaign.rb"
  m.line          # => 42
  m.return_type   # => "T.any(FalseClass, TrueClass)"
  m.uses_block    # => false
  m.rescues       # => ["ActiveRecord::RecordNotFound"]

  m.arguments.each do |arg|
    arg.name       # => "at"
    arg.type       # => "T.untyped"
    arg.optional?  # => true
  end

  m.calls.each do |call|
    call.receiver_type  # => "Campaign"
    call.method_name    # => "where"
    call.return_type    # => "T.untyped"
    call.conditions     # => [#<SrbLens::BranchCondition ...>]
  end

  m.ivars.each do |ivar|
    ivar.name  # => "@count"
    ivar.type  # => "Integer"
  end

  m.basic_blocks.each do |bb|
    bb.id          # => 0
    bb.terminator  # => "branch <self>.nil?() ? bb1 : bb2"
  end
end
```

### Querying classes

```ruby
classes = project.find_classes("Campaign")

classes.each do |c|
  c.fqn          # => "Campaign"
  c.is_module    # => false
  c.super_class  # => "ApplicationRecord"
  c.mixins       # => ["Multitenancy", "Archivable"]
  c.file_path    # => "app/models/campaign.rb"
  c.line         # => 3
end
```

## API Reference

### `SrbLens::Project`

| Method | Description |
|--------|-------------|
| `.load_from_cache(dir)` | Load project from existing `.srb-lens/` cache |
| `.load_or_index(dir, srb_command: nil)` | Load from cache or run Sorbet to index |
| `.index(dir, srb_command: nil)` | Always run Sorbet and rebuild the index |
| `#find_methods(query)` | Search methods by `"Class#method"`, `"Class.method"`, or `"Class"` |
| `#find_classes(query)` | Search classes/modules by name (partial match) |

### `SrbLens::MethodInfo`

| Method | Return Type | Description |
|--------|-------------|-------------|
| `#fqn` | `String` | Fully qualified name (e.g. `"Foo#bar"`) |
| `#file_path` | `String?` | Source file path relative to project root |
| `#line` | `Integer?` | Line number of the method definition |
| `#return_type` | `String?` | Sorbet return type |
| `#arguments` | `Array<Argument>` | Method parameters |
| `#calls` | `Array<MethodCall>` | Method calls within the body |
| `#ivars` | `Array<IvarAccess>` | Instance variable accesses |
| `#rescues` | `Array<String>` | Exception types rescued |
| `#uses_block` | `Boolean` | Whether the method accepts a block |
| `#basic_blocks` | `Array<BasicBlock>` | CFG basic blocks |

### `SrbLens::ClassInfo`

| Method | Return Type | Description |
|--------|-------------|-------------|
| `#fqn` | `String` | Fully qualified name |
| `#is_module` | `Boolean` | `true` if module, `false` if class |
| `#super_class` | `String?` | Parent class FQN |
| `#mixins` | `Array<String>` | Included/extended modules |
| `#file_path` | `String?` | Source file path |
| `#line` | `Integer?` | Definition line number |

### `SrbLens::Argument`

| Method | Return Type | Description |
|--------|-------------|-------------|
| `#name` | `String` | Parameter name |
| `#type` | `String` | Sorbet type annotation |
| `#optional?` | `Boolean` | Whether the parameter is optional |

### `SrbLens::MethodCall`

| Method | Return Type | Description |
|--------|-------------|-------------|
| `#receiver_type` | `String` | Type of the receiver |
| `#method_name` | `String` | Name of the called method |
| `#return_type` | `String` | Return type of the call |
| `#conditions` | `Array<BranchCondition>` | Branch conditions to reach this call |

### `SrbLens::BranchCondition`

| Method | Return Type | Description |
|--------|-------------|-------------|
| `#call` | `String` | Description of the condition |
| `#true?` | `Boolean` | Whether this is the true branch |

### `SrbLens::IvarAccess`

| Method | Return Type | Description |
|--------|-------------|-------------|
| `#name` | `String` | Instance variable name (e.g. `"@count"`) |
| `#type` | `String` | Sorbet type annotation |

### `SrbLens::BasicBlock`

| Method | Return Type | Description |
|--------|-------------|-------------|
| `#id` | `Integer` | Block ID |
| `#terminator` | `String` | How the block ends (e.g. `"return"`, `"branch ..."`) |

## How It Works

The native extension is built with [rb_sys](https://github.com/oxidize-rb/rb-sys) and [Magnus](https://github.com/matsadler/magnus). At `gem install` / `rake compile` time, Cargo compiles the Rust code into a shared library (`.bundle` on macOS, `.so` on Linux) that Ruby loads in-process.

Under the hood, srb-lens runs Sorbet with `--print` flags to produce:

1. **cfg-text** &mdash; control flow graphs for every method
2. **symbol-table-json** &mdash; class hierarchy, mixins, method signatures
3. **autogen** &mdash; file paths and line numbers for definitions
4. **parse-tree-json-with-locs** &mdash; precise method definition locations

These are cached in a `.srb-lens/` directory inside the target project for fast subsequent loads.
