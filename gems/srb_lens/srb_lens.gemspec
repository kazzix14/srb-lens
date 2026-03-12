Gem::Specification.new do |spec|
  spec.name = "srb_lens"
  spec.version = "0.4.0"
  spec.authors = ["kazzix14"]
  spec.email = ["kazzix14@gmail.com"]
  spec.summary = "Ruby bindings for srb-lens (Sorbet code analysis)"
  spec.description = "Extract method signatures, call graphs, and type information from Sorbet-typed Ruby projects"
  spec.homepage = "https://github.com/kazzix14/srb-lens"
  spec.license = "MIT"

  spec.metadata = {
    "homepage_uri" => spec.homepage,
    "source_code_uri" => spec.homepage,
    "changelog_uri" => "#{spec.homepage}/blob/main/CHANGELOG.md",
  }

  spec.files = Dir["lib/**/*.rb", "ext/**/*.{rs,toml,rb}"]
  spec.extensions = ["ext/srb_lens/extconf.rb"]
  spec.require_paths = ["lib"]

  spec.required_ruby_version = ">= 3.1"

  spec.add_dependency "rb_sys", ">= 0.9.124"
end
