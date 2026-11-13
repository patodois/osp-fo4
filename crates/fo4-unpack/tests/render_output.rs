#![forbid(unsafe_code)]
#[allow(dead_code)]
#[path = "../examples/support/mod.rs"]
mod files;

#[test]
fn render_output_rejects_input_root_before_creating_any_directory() {
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    let data = cwd.join("build/synthetic-readonly-input");
    let out = data.join("must-not-create");
    assert!(!out.exists());
    let result = files::output_dir_away_from(out.to_str().unwrap(), Some(&data));
    assert_eq!(
        result.unwrap_err().to_string(),
        "RenderOutputInsideInputRoot"
    );
    assert!(!out.exists());
}

#[test]
fn render_output_rejects_traversal_and_nonbuild_paths_before_writing() {
    for path in ["build/../outside", "outside"] {
        assert!(files::output_dir_away_from(path, None).is_err());
    }
}
