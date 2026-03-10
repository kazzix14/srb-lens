Gem::Specification.new do |spec|
  spec.name = "srb_lens"
  spec.version = "0.1.0"
  spec.authors = ["kazuma"]
  spec.summary = "Ruby bindings for srb-lens (Sorbet code analysis)"

  spec.files = Dir["lib/**/*.rb", "ext/**/*.{rs,toml,rb}"]
  spec.extensions = ["ext/srb_lens/extconf.rb"]
  spec.require_paths = ["lib"]

  spec.required_ruby_version = ">= 3.1"

  spec.add_dependency "rb_sys", ">= 0.9.124"
end
