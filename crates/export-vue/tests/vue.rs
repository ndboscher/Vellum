use vellum_export_core::{ExportError, ExportOptions, ExportResult, Exporter};
use vellum_export_vue::{export_vue, VueExporter, VueOptions};
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
fn emits_single_file_components_per_page_and_component() {
    let result = export_vue(&site(), &VueOptions::default()).unwrap();
    assert_eq!(
        paths(&result),
        [
            "src/App.vue",
            "src/pages/Home.vue",
            "src/pages/PricingPlans.vue",
            "src/components/PlanCard.vue",
            "src/styles.css"
        ]
    );
    insta::assert_snapshot!("App.vue", result.file("src/App.vue").unwrap().contents);
    insta::assert_snapshot!(
        "PlanCard.vue",
        result.file("src/components/PlanCard.vue").unwrap().contents
    );
    insta::assert_snapshot!(
        "PricingPlans.vue",
        result.file("src/pages/PricingPlans.vue").unwrap().contents
    );
}

#[test]
fn shares_the_stylesheet_with_html() {
    let vue = export_vue(&site(), &VueOptions::default()).unwrap();
    let html = vellum_export_html::export_html(&site(), &Default::default());
    assert_eq!(
        vue.file("src/styles.css").unwrap().contents,
        html.file("styles.css").unwrap().contents
    );
}

#[test]
fn keeps_template_syntax_in_text_inert() {
    let result = export_vue(&landing(), &VueOptions::default()).unwrap();
    let home = &result.file("src/pages/Home.vue").unwrap().contents;
    assert!(home.contains("Build sites &lt;fast&gt;"));
    assert!(home.contains("draw a &#123;logo}, export."));
    assert!(home.contains(r#"alt="Screenshot of the &quot;editor&quot;""#));
    assert!(home.contains(r#"stroke-width="2""#));
    assert!(
        !home.contains("<script"),
        "pages without components need no script"
    );
}

#[test]
fn reads_options_through_the_exporter_trait() {
    let mut options = ExportOptions::new();
    options.insert("typescript".into(), true.into());
    options.insert("componentName".into(), "Site".into());
    let result = VueExporter.export(&site(), &options).unwrap();
    let root = &result.file("src/Site.vue").unwrap().contents;
    assert!(root.starts_with(r#"<script setup lang="ts">"#));

    let mut bad = ExportOptions::new();
    bad.insert("componentName".into(), "my app".into());
    assert!(matches!(
        VueExporter.export(&site(), &bad),
        Err(ExportError::InvalidOptions { .. })
    ));
}
