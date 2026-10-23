use vellum_export_core::{ExportOptions, ExportResult, Exporter};
use vellum_export_react::{export_react, ReactExporter, ReactOptions};
use vellum_schema::Document;

fn landing() -> Document {
    serde_json::from_str(include_str!("../../export-core/fixtures/landing.json"))
        .expect("fixture page parses")
}

fn site() -> Document {
    serde_json::from_str(include_str!("../../export-core/fixtures/site.json"))
        .expect("fixture site parses")
}

fn paths(result: &ExportResult) -> Vec<&str> {
    result.files.iter().map(|f| f.path.as_str()).collect()
}

#[test]
fn emits_component_and_stylesheet() {
    let result = export_react(&landing(), &ReactOptions::default()).unwrap();
    assert_eq!(
        paths(&result),
        ["src/App.jsx", "src/pages/Home.jsx", "src/styles.css"]
    );
    insta::assert_snapshot!(
        "Home.jsx",
        result.file("src/pages/Home.jsx").unwrap().contents
    );
}

#[test]
fn shares_the_stylesheet_with_html() {
    let react = export_react(&landing(), &ReactOptions::default()).unwrap();
    let html = vellum_export_html::export_html(&landing(), &Default::default());
    assert_eq!(
        react.file("src/styles.css").unwrap().contents,
        html.file("styles.css").unwrap().contents
    );
}

#[test]
fn uses_react_attribute_names() {
    let result = export_react(&landing(), &ReactOptions::default()).unwrap();
    let jsx = &result.file("src/pages/Home.jsx").unwrap().contents;
    assert!(jsx.contains(r#"className="page""#));
    assert!(jsx.contains(r#"strokeWidth="2""#));
    assert!(jsx.contains(r#"strokeLinecap="round""#));
    assert!(jsx.contains(r#"aria-label="Vector graphic""#));
    assert!(!jsx.contains(" class=") && !jsx.contains(" stroke-width="));
}

#[test]
fn keeps_jsx_special_characters_as_text() {
    let result = export_react(&landing(), &ReactOptions::default()).unwrap();
    let jsx = &result.file("src/pages/Home.jsx").unwrap().contents;
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
    assert_eq!(
        paths(&result),
        ["src/Landing.tsx", "src/pages/Home.tsx", "src/styles.css"]
    );
    let tsx = &result.file("src/Landing.tsx").unwrap().contents;
    assert!(
        tsx.starts_with("import type { ComponentType } from \"react\";\nimport \"./styles.css\";")
    );
    assert!(tsx.contains("const routes: Record<string, ComponentType> = {"));
    assert!(tsx.contains("export default function Landing() {"));
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

#[test]
fn routes_each_page_by_path() {
    let result = export_react(&site(), &ReactOptions::default()).unwrap();
    assert_eq!(
        paths(&result),
        [
            "src/App.jsx",
            "src/pages/Home.jsx",
            "src/pages/PricingPlans.jsx",
            "src/components/PlanCard.jsx",
            "src/styles.css"
        ]
    );
    insta::assert_snapshot!("site-App.jsx", result.file("src/App.jsx").unwrap().contents);
    let pricing = &result.file("src/pages/PricingPlans.jsx").unwrap().contents;
    assert!(pricing.contains("\nexport default function PricingPlans() {"));
    assert!(pricing.contains(r#"<a className="blk-nav-link" href="/pricing" aria-current="page">"#));
    let home = &result.file("src/pages/Home.jsx").unwrap().contents;
    assert!(home.contains(r##"href="/pricing#plans""##));
}

#[test]
fn page_components_never_clash_with_the_root() {
    let mut doc = site();
    doc.pages[1].name = "App".into();
    let result = export_react(&doc, &ReactOptions::default()).unwrap();
    assert!(result.file("src/pages/App2.jsx").is_some());
    doc.pages[1].name = "404".into();
    let result = export_react(&doc, &ReactOptions::default()).unwrap();
    assert!(result.file("src/pages/Page404.jsx").is_some());
}

#[test]
fn components_become_files_with_props() {
    let result = export_react(&site(), &ReactOptions::default()).unwrap();
    let card = &result.file("src/components/PlanCard.jsx").unwrap().contents;
    insta::assert_snapshot!("site-PlanCard.jsx", card);
    assert!(card.starts_with(
        r#"export default function PlanCard({ title = "Starter", price = "Free", signup = "/" }) {"#
    ));
    let pricing = &result.file("src/pages/PricingPlans.jsx").unwrap().contents;
    assert!(pricing.starts_with("import PlanCard from \"../components/PlanCard\";\n\n"));
    assert!(pricing.contains("<PlanCard />"));
    assert!(
        pricing.contains(r##"<PlanCard title="Pro" price="$12 a month" signup="/pricing#pro" />"##)
    );

    let options = ReactOptions {
        typescript: true,
        ..ReactOptions::default()
    };
    let result = export_react(&site(), &options).unwrap();
    let card = &result.file("src/components/PlanCard.tsx").unwrap().contents;
    assert!(card.contains("}: { title?: string; price?: string; signup?: string }) {"));
}
