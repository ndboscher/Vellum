use vellum_export_core::{ExportOptions, ExportResult, Exporter};
use vellum_export_react::{export_react, ReactExporter, ReactOptions};
use vellum_schema::Document;

fn landing() -> Document {
    serde_json::from_str(include_str!("../../export-core/fixtures/landing.json"))
        .expect("fixture page parses")
}

fn paths(result: &ExportResult) -> Vec<&str> {
    result.files.iter().map(|f| f.path.as_str()).collect()
}

#[test]
fn emits_component_and_stylesheet() {
    let result = export_react(&landing(), &ReactOptions::default()).unwrap();
    assert_eq!(paths(&result), ["src/Page.jsx", "src/Page.css"]);
    insta::assert_snapshot!("Page.jsx", result.file("src/Page.jsx").unwrap().contents);
}

#[test]
fn shares_the_stylesheet_with_html() {
    let react = export_react(&landing(), &ReactOptions::default()).unwrap();
    let html = vellum_export_html::export_html(&landing(), &Default::default());
    assert_eq!(
        react.file("src/Page.css").unwrap().contents,
        html.file("styles.css").unwrap().contents
    );
}

#[test]
fn uses_react_attribute_names() {
    let result = export_react(&landing(), &ReactOptions::default()).unwrap();
    let jsx = &result.file("src/Page.jsx").unwrap().contents;
    assert!(jsx.contains(r#"className="page""#));
    assert!(jsx.contains(r#"strokeWidth="2""#));
    assert!(jsx.contains(r#"strokeLinecap="round""#));
    assert!(jsx.contains(r#"aria-label="Vector graphic""#));
    assert!(!jsx.contains(" class=") && !jsx.contains(" stroke-width="));
}

#[test]
fn keeps_jsx_special_characters_as_text() {
    let result = export_react(&landing(), &ReactOptions::default()).unwrap();
    let jsx = &result.file("src/Page.jsx").unwrap().contents;
    assert!(jsx.contains(r#"{"Build sites <fast>"}"#));
    assert!(jsx.contains(r#"{"Drag blocks, draw a {logo}, export.\nThen ship it."}"#));
    assert!(jsx.contains(r#"alt={"Screenshot of the \"editor\""}"#));
}

#[test]
fn typescript_and_component_name_options() {
    let mut options = ExportOptions::new();
    options.insert("componentName".into(), "Landing".into());
    options.insert("typescript".into(), true.into());
    let result = ReactExporter.export(&landing(), &options).unwrap();
    assert_eq!(paths(&result), ["src/Landing.tsx", "src/Landing.css"]);
    let tsx = &result.file("src/Landing.tsx").unwrap().contents;
    assert!(tsx.starts_with("import \"./Landing.css\";\n\nexport default function Landing() {"));
}

#[test]
fn rejects_invalid_component_names() {
    let options = ReactOptions {
        component_name: "my page".into(),
        typescript: false,
    };
    let err = export_react(&landing(), &options).unwrap_err();
    assert!(err.to_string().contains("PascalCase"), "{err}");
}
