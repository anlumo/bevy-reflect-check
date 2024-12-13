use cargo_metadata::camino::Utf8PathBuf;
use cargo_metadata::{Metadata, MetadataCommand};
use std::collections::HashMap;
use std::{fs, path::Path};
use syn::{Attribute, File, Item, Meta, Visibility, parse_file};
use walkdir::{DirEntry, WalkDir};

fn main() {
    // Fetch metadata for dependency crates
    let metadata = MetadataCommand::new()
        .exec()
        .expect("Failed to fetch cargo metadata");

    // Collect all source files from the current project and dependencies
    let mut source_files = Vec::new();
    collect_source_files("./src", &mut source_files); // Scan only `src` in the current project
    collect_dependency_files(&metadata, &mut source_files); // Dependencies

    // Build a module hierarchy
    let mut module_tree = HashMap::new();
    for path in &source_files {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(syntax) = parse_file(&content) {
                build_module_tree(path, &syntax, &mut module_tree);
            }
        }
    }

    // Track collected types with fully qualified paths
    let mut reflect_types = Vec::new();
    for (path, syntax) in &module_tree {
        if let Some(module_path) = resolve_module_path(path, &metadata) {
            collect_reflect_types(
                syntax,
                &module_path,
                &mut reflect_types,
                /* public_only = */ true,
                /* parent_is_public = */ true,
            );
        }
    }
    println!("{:?}", reflect_types);
}

// Recursively collect all `.rs` files in a directory, excluding `examples` and `tests`
fn collect_source_files(dir: &str, source_files: &mut Vec<String>) {
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(should_include_dir)
        .filter_map(|e| e.ok())
    {
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("rs") {
            source_files.push(entry.path().to_string_lossy().into_owned());
        }
    }
}

// Exclude `examples` and `tests` directories
fn should_include_dir(entry: &DirEntry) -> bool {
    let path = entry.path();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    !(name == "examples" || name == "tests")
}

// Collect `.rs` files from dependencies
fn collect_dependency_files(metadata: &Metadata, source_files: &mut Vec<String>) {
    for package in &metadata.packages {
        if package.name.starts_with("bevy_") {
            if let Some(source) = package.manifest_path.parent() {
                collect_source_files(source.as_str(), source_files);
            }
        }
    }
}

// Parse the module hierarchy from `mod` declarations
fn build_module_tree(path: &str, file: &File, module_tree: &mut HashMap<String, File>) {
    module_tree.insert(path.to_string(), file.clone());
}

// Resolve the fully qualified module path from a file's relative path
fn resolve_module_path(path: &str, metadata: &Metadata) -> Option<String> {
    let path = Path::new(path);

    if let Some(crate_name) = crate_root_for_file(path, metadata) {
        let relative_path = path
            .strip_prefix(crate_root_path(&crate_name, metadata)?)
            .ok()?;
        let module_path = relative_path_to_module_path(relative_path);
        Some(format!("{}::{}", crate_name, module_path))
    } else {
        let relative_path = path.strip_prefix("src").ok()?;
        Some(relative_path_to_module_path(relative_path))
    }
}

// Find the crate name for a given file
fn crate_root_for_file(path: &Path, metadata: &Metadata) -> Option<String> {
    for package in &metadata.packages {
        let crate_root = Path::new(&package.manifest_path).parent()?;
        if path.starts_with(crate_root) {
            return Some(package.name.clone());
        }
    }
    None
}

// Get the root path of a crate
fn crate_root_path(crate_name: &str, metadata: &Metadata) -> Option<Utf8PathBuf> {
    metadata
        .packages
        .iter()
        .find(|pkg| pkg.name == crate_name)
        .and_then(|pkg| pkg.manifest_path.parent().map(|p| p.to_path_buf()))
}

// Convert a relative path to a Rust module path
fn relative_path_to_module_path(path: &Path) -> String {
    path.iter()
        .filter_map(|comp| comp.to_str())
        .map(|s| s.trim_end_matches(".rs"))
        .filter(|s| *s != "mod")
        .collect::<Vec<_>>()
        .join("::")
}

// Check if a struct or module has the `#[cfg(test)]` attribute
fn has_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if let syn::Meta::List(meta_list) = &attr.meta {
            return meta_list.path.is_ident("cfg") && meta_list.tokens.to_string().contains("test");
        }
        false
    })
}

/// Check if an item is public.
fn is_public(item: &Item) -> bool {
    match item {
        Item::Struct(s) => matches!(s.vis, Visibility::Public(_)),
        Item::Enum(e) => matches!(e.vis, Visibility::Public(_)),
        Item::Mod(m) => matches!(m.vis, Visibility::Public(_)),
        _ => false,
    }
}

/// Checks if a struct derives `Reflect` and `Component` but does not have `#[reflect(Component)]`.
fn derives_reflect_and_component_but_no_reflect_component(attrs: &[Attribute]) -> bool {
    let mut derives_reflect = false;
    let mut derives_component = false;
    let mut has_reflect_component_attr = false;

    for attr in attrs {
        match &attr.meta {
            // Check for `#[derive(...)]`
            Meta::List(meta_list) if meta_list.path.is_ident("derive") => {
                meta_list
                    .parse_nested_meta(|nested_meta| {
                        if nested_meta.path.is_ident("Reflect") {
                            derives_reflect = true;
                        } else if nested_meta.path.is_ident("Component") {
                            derives_component = true;
                        }
                        Ok(())
                    })
                    .ok();
            }
            // Check for `#[reflect(Component)]`
            Meta::List(meta_list) if meta_list.path.is_ident("reflect") => {
                meta_list
                    .parse_nested_meta(|nested_meta| {
                        if nested_meta.path.is_ident("Component") {
                            has_reflect_component_attr = true;
                        }
                        Ok(())
                    })
                    .ok();
            }
            _ => {}
        }
    }

    derives_reflect && derives_component && !has_reflect_component_attr
}

/// Recursively collect `#[derive(Reflect)]` types while respecting visibility.
fn collect_reflect_types(
    file: &File,
    module_path: &str,
    reflect_types: &mut Vec<String>,
    public_only: bool,
    parent_is_public: bool,
) {
    for item in &file.items {
        let item_is_public = is_public(item) && parent_is_public;
        match item {
            Item::Struct(s) if derives_reflect_and_component_but_no_reflect_component(&s.attrs) => {
                if public_only && !item_is_public {
                    continue;
                }
                let full_path = format!("{}::{}", module_path, s.ident);
                reflect_types.push(full_path);
            }
            Item::Enum(s) if derives_reflect_and_component_but_no_reflect_component(&s.attrs) => {
                if public_only && !item_is_public {
                    continue;
                }
                let full_path = format!("{}::{}", module_path, s.ident);
                reflect_types.push(full_path);
            }
            Item::Mod(m) if !has_cfg_test(&m.attrs) => {
                if public_only && !item_is_public {
                    continue;
                }
                if let Some((_, items)) = &m.content {
                    let nested_path = format!("{}::{}", module_path, m.ident);
                    let nested_file = File {
                        items: items.clone(),
                        attrs: vec![],
                        shebang: None,
                    };
                    collect_reflect_types(
                        &nested_file,
                        &nested_path,
                        reflect_types,
                        public_only,
                        item_is_public,
                    );
                }
            }
            _ => {}
        }
    }
}
