use std::path::Path;

#[cfg(not(debug_assertions))]
use rust_embed::Embed;
use skabelon::Templates;

#[cfg(not(debug_assertions))]
#[derive(Embed)]
#[folder = "templates"]
struct TemplatesEmbed;

#[allow(unused_variables)]
pub fn templates(root_dir: &Path) -> Templates {
    let mut templates = Templates::new();

    #[cfg(not(debug_assertions))]
    {
        for file in TemplatesEmbed::iter() {
            let content = TemplatesEmbed::get(&file).unwrap();
            let content = String::from_utf8_lossy(&content.data);
            templates.load_str(&file, &content);
        }
    }

    #[cfg(debug_assertions)]
    {
        let dir = format!("{}/**/*.html", root_dir.to_str().unwrap());
        templates.load_glob(&dir);
    }
    templates
}
