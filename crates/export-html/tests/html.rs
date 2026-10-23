use vellum_export_core::{ExportError, ExportOptions, Exporter};
use vellum_export_html::{export_html, HtmlExporter, HtmlOptions};
use vellum_schema::Document;

fn landing() -> Document {
    serde_json::from_str(include_str!("../../export-core/fixtures/landing.json"))
        .expect("fixture page parses")
}

fn site() -> Document {
    serde_json::from_str(include_str!("../../export-core/fixtures/site.json"))
        .expect("fixture site parses")
}

#[test]
fn emits_page_and_stylesheet() {
    let result = export_html(&landing(), &HtmlOptions::default());
    let paths: Vec<_> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["index.html", "styles.css"]);
    insta::assert_snapshot!("index.html", result.file("index.html").unwrap().contents);
    insta::assert_snapshot!("styles.css", result.file("styles.css").unwrap().contents);
}

#[test]
fn escapes_text_and_attributes() {
    let result = export_html(&landing(), &HtmlOptions::default());
    let html = &result.file("index.html").unwrap().contents;
    assert!(html.contains("<title>Alpaca &amp; Co</title>"));
    assert!(html.contains("Build sites &lt;fast&gt;"));
    assert!(html.contains(r#"alt="Screenshot of the &quot;editor&quot;""#));
}

#[test]
fn can_inline_the_stylesheet() {
    let result = export_html(&landing(), &HtmlOptions { inline_css: true });
    assert_eq!(result.files.len(), 1);
    let html = &result.file("index.html").unwrap().contents;
    assert!(html.contains("<style>\n    :root {\n      --color-text: #1f1b2e;"));
    assert!(!html.contains("styles.css"));
}

#[test]
fn reads_options_through_the_exporter_trait() {
    let mut options = ExportOptions::new();
    options.insert("inlineCss".into(), true.into());
    assert_eq!(
        HtmlExporter
            .export(&landing(), &options)
            .unwrap()
            .files
            .len(),
        1
    );

    let mut typo = ExportOptions::new();
    typo.insert("inlineCSS".into(), true.into());
    assert!(matches!(
        HtmlExporter.export(&landing(), &typo),
        Err(ExportError::InvalidOptions { .. })
    ));
}

#[test]
fn neutralizes_script_links() {
    let mut doc = landing();
    if let vellum_schema::NodeKind::Button { href, .. } = &mut doc.pages[0].nodes[2].kind {
        *href = "javascript:alert(1)".into();
    }
    let result = export_html(&doc, &HtmlOptions::default());
    let html = &result.file("index.html").unwrap().contents;
    assert!(html.contains(r##"href="#""##));
    assert!(!html.contains("javascript:"));
}

#[test]
fn writes_a_file_per_page_with_links_between_them() {
    let result = export_html(&site(), &HtmlOptions::default());
    let paths: Vec<_> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["index.html", "pricing.html", "styles.css"]);
    let home = &result.file("index.html").unwrap().contents;
    let pricing = &result.file("pricing.html").unwrap().contents;
    insta::assert_snapshot!("site-pricing.html", pricing);
    assert!(home.contains("<title>Alpaca &amp; Co</title>"));
    assert!(pricing.contains("<title>Pricing &amp; Plans · Alpaca &amp; Co</title>"));
    assert!(home.contains(r##"href="pricing.html#plans""##));
    assert!(pricing.contains(r#"<a class="blk-button" href="index.html">Back home</a>"#));
    assert!(
        home.contains(r#"<a class="blk-nav-link" href="index.html" aria-current="page">Home</a>"#)
    );
    assert!(pricing.contains(r#"<a class="blk-nav-link" href="index.html">Home</a>"#));
    // Component instances print as plain markup with their props filled in.
    assert!(pricing.contains(r#"<h2 class="blk-heading heading-3">Pro</h2>"#));
    assert!(pricing.contains(r##"href="pricing.html#pro""##));
    assert_eq!(
        pricing
            .matches(r#"<div class="blk-component plan-card">"#)
            .count(),
        2
    );
}

#[test]
fn emits_breakpoint_overrides_as_media_queries() {
    let result = export_html(&site(), &HtmlOptions::default());
    let css = &result.file("styles.css").unwrap().contents;
    insta::assert_snapshot!("site-styles.css", css);
    assert!(css.contains("@media (max-width: 900px) {\n  .grid {\n    grid-template-columns: repeat(2, minmax(0, 1fr));"));
    assert!(css.contains("  }\n\n  .spacer {\n    display: none;\n  }\n"));
}

#[test]
fn animations_export_as_keyframes() {
    let result = export_html(&site(), &HtmlOptions::default());
    let css = &result.file("styles.css").unwrap().contents;
    let home = &result.file("index.html").unwrap().contents;
    assert!(css.contains("  animation: heading-anim 0.6s ease-out 0.1s 1 normal both;"));
    assert!(css.contains("@keyframes heading-anim {\n  0% {\n    opacity: 0;\n    transform: translate(0px, 24px) rotate(0deg) scale(1);"));
    assert!(css.contains(".shape {\n  animation: shape-anim 4s linear 0s infinite normal both;\n  transform-box: fill-box;"));
    assert!(css.ends_with("@media (prefers-reduced-motion: reduce) {\n  .heading,\n  .shape {\n    animation: none;\n  }\n}\n"));
    assert!(home.contains(r#"<rect class="shape" "#));
}
