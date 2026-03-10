# srb-lens

A static analysis toolkit for [Sorbet](https://sorbet.org/)-typed Ruby projects. srb-lens parses Sorbet's internal outputs (CFG, symbol tables, autogen) and builds a queryable model of your project's methods, classes, types, and control flow.

## What it extracts

For every method in your project, srb-lens gives you:

- **Signature** &mdash; arguments with types, return type, optional parameters
- **Call graph** &mdash; what methods are called, on what receiver types, with what return types
- **Control flow** &mdash; basic blocks, branch conditions, which calls happen under which branches
- **Class hierarchy** &mdash; superclasses, mixins, modules
- **Instance variables** &mdash; names and types
- **Source locations** &mdash; file paths and line numbers
- **Block usage & rescue clauses**

## Project structure

```
srb-lens/
  crates/
    srb-lens/          # Core Rust library
    srb-lens-cli/      # Command-line interface
  gems/
    srb_lens/          # Ruby gem (native extension via Magnus)
```

## Getting started

### Prerequisites

- Rust toolchain (stable, edition 2024)
- A Ruby project with [Sorbet](https://sorbet.org/) configured

### Install the CLI

```bash
cargo install --path crates/srb-lens-cli
```

### Index a project

```bash
cd /path/to/your-ruby-project
srb-lens index
```

This runs Sorbet with `--print` flags, parses the output, and caches everything in a `.srb-lens/` directory. If your project uses Bundler:

```bash
srb-lens index --srb-command "bundle exec srb"
```

### Query methods

```bash
# Instance method
srb-lens query "User#activate!"

# Class method
srb-lens query "User.find_by_email"

# All methods in a class
srb-lens query "User"

# List matching method names only
srb-lens query "User" --list

# JSON output
srb-lens query "User#activate!" --json
```

### Pipe mode

You can pipe Sorbet's cfg-text output directly:

```bash
srb tc --print=cfg-text 2>/dev/null | srb-lens pipe "User#activate!"
```

## CLI reference

### `srb-lens index`

Run Sorbet and build the index cache.

| Flag | Description |
|------|-------------|
| `-d, --dir <DIR>` | Project root (default: current directory) |
| `--srb-command <CMD>` | Sorbet command (default: `srb`) |

### `srb-lens query <QUERY>`

Query method or class information from the index.

| Flag | Description |
|------|-------------|
| `-d, --dir <DIR>` | Project root (default: current directory) |
| `--index` | Force re-index before querying |
| `--srb-command <CMD>` | Sorbet command (used with `--index`) |
| `--cfg <FILE>` | Read cfg-text from file instead of cache |
| `--symbols <FILE>` | Read symbol-table-json from file |
| `--autogen <FILE>` | Read autogen from file |
| `--list` | List matching method names only |
| `--json` | Output as JSON |

### `srb-lens pipe <QUERY>`

Read cfg-text from stdin and query.

| Flag | Description |
|------|-------------|
| `--list` | List matching method names only |
| `--json` | Output as JSON |

### Query format

| Pattern | Meaning |
|---------|---------|
| `Foo#bar` | Instance method `bar` on `Foo` |
| `Foo.bar` | Class method `bar` on `Foo` |
| `Foo` | All methods defined in `Foo` |

Partial class names are supported &mdash; `Campaign` matches `Marketing::Campaign`.
Inherited methods are resolved by walking the superclass chain.

## Example output

```
== Campaign#active? ==
  class: Campaign < ApplicationRecord
  mixins: Multitenancy, Archivable
  defined: app/models/campaign.rb:3
  source: app/models/campaign.rb:42
  args:
    at: Time (optional)
  returns: T.any(FalseClass, TrueClass)
  calls:
    Campaign.starts_at() -> T.nilable(Time)
    Campaign.ends_at() -> T.nilable(Time)
    Time.now() -> Time  when: <self>.starts_at() = true
  cfg:
    bb0 -> bb1
    bb1 -[<self>.starts_at()]-> true:bb2 / false:bb3
    bb2 -> return
    bb3 -> return
```

## Ruby gem

The `srb_lens` Ruby gem provides in-process access to the same analysis engine via a native extension (no subprocess, no JSON serialization overhead).

```ruby
require "srb_lens"

project = SrbLens::Project.load_or_index("/path/to/project", srb_command: "bundle exec srb")

project.find_methods("Campaign#active?").each do |m|
  puts "#{m.fqn} -> #{m.return_type}"
  m.calls.each { |c| puts "  calls #{c.receiver_type}.#{c.method_name}" }
end

project.find_classes("Campaign").each do |c|
  puts "#{c.fqn} < #{c.super_class}  mixins: #{c.mixins.join(', ')}"
end
```

See [gems/srb_lens/README.md](gems/srb_lens/README.md) for the full Ruby API reference.

### Building the gem

```bash
cd gems/srb_lens
bundle install
bundle exec rake compile
```

## How it works

srb-lens runs Sorbet with four `--print` flags and combines the results:

| Sorbet output | What srb-lens extracts |
|---------------|----------------------|
| `--print=cfg-text` | Control flow graphs: basic blocks, method calls, branch conditions, return types, block usage |
| `--print=symbol-table-json` | Class hierarchy: superclasses, mixins, method signatures |
| `--print=autogen --stop-after=namer` | File paths and line numbers for class/module definitions |
| `--print=parse-tree-json-with-locs --stop-after=parser` | Precise method definition locations |

All outputs are cached in `.srb-lens/` for fast subsequent loads.

## License

[MIT](LICENSE)
