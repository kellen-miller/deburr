//! Renderer tests that cover several formats at once: JSON, ledgers, and
//! the incompatible-comparison wording.

mod render_fixture;

use deburr::render::{self, Format, Source, Style};
use deburr::report::{Comparison, Report};
use render_fixture::{
    FailingWriter, comparison_in, report_in, sample_comparison, sample_report,
    sample_report_with_functions,
};

#[test]
fn json_round_trips_many_functions_in_order() {
    let report = sample_report_with_functions(25);
    let decoded: Report = serde_json::from_str(&report_in(&report, Format::Json)).expect("decode");

    assert_eq!(decoded, report);
    assert_eq!(decoded.functions[24].name, "F24");
}

#[test]
fn json_reports_writer_errors() {
    let source = Source::default();

    assert!(
        render::render_report(
            &sample_report(),
            &source,
            Style::default(),
            Format::Json,
            &mut FailingWriter
        )
        .is_err(),
        "json swallowed a report write error"
    );
    assert!(
        render::render_comparison(
            &sample_comparison(),
            &source,
            Style::default(),
            Format::Json,
            &mut FailingWriter
        )
        .is_err(),
        "json swallowed a comparison write error"
    );
}

#[test]
fn json_round_trips_the_report() {
    let report = sample_report();
    let output = report_in(&report, Format::Json);
    let decoded: Report = serde_json::from_str(&output).expect("decode");

    assert_eq!(decoded, report);
    assert!(output.ends_with('\n'));
}

#[test]
fn json_comparison_round_trips() {
    let comparison = sample_comparison();
    let output = comparison_in(&comparison, Format::Json);
    let decoded: Comparison = serde_json::from_str(&output).expect("decode");

    assert_eq!(decoded, comparison);
}

#[test]
fn source_links_are_permalinks_with_line_ranges() {
    let source = Source::new(
        None,
        Some("https://github.com/o/r/blob/abc".to_owned()),
        "services/api/".to_owned(),
    );

    assert_eq!(
        source.link("pkg/a b.go", 3, 9).as_deref(),
        Some("https://github.com/o/r/blob/abc/services/api/pkg/a%20b.go#L3-L9")
    );
    assert_eq!(
        source.link("a.go", 4, 4).as_deref(),
        Some("https://github.com/o/r/blob/abc/services/api/a.go#L4")
    );
    assert_eq!(Source::default().link("a.go", 1, 2), None);
    assert_eq!(Source::default().name(), "report");
    assert_eq!(Source::default().named("repo").name(), "repo");
}

#[test]
fn source_lines_are_read_from_inside_the_root_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
    std::fs::write(dir.path().join("sub/a.go"), "one\r\ntwo\nthree\n").expect("write");
    let source = Source::new(Some(dir.path().to_path_buf()), None, String::new());

    assert_eq!(
        source.lines("sub/a.go", 2, 3),
        Some(vec!["two".to_owned(), "three".to_owned()])
    );
    assert_eq!(source.lines("sub/a.go", 1, 1), Some(vec!["one".to_owned()]));
    assert_eq!(
        source.lines("sub/a.go", 2, 4),
        None,
        "file is shorter than end"
    );
    assert_eq!(source.lines("sub/a.go", 0, 1), None);
    assert_eq!(source.lines("sub/a.go", 3, 2), None);
    assert_eq!(source.lines("sub/missing.go", 1, 1), None);
    assert_eq!(
        source.lines("../sub/a.go", 1, 1),
        None,
        "no escaping the root"
    );
    assert_eq!(source.lines("/etc/passwd", 1, 1), None);
    assert_eq!(Source::default().lines("sub/a.go", 1, 1), None);
}

#[cfg(unix)]
#[test]
fn source_lines_refuse_symlinks_that_leave_the_root() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("tempdir");
    std::fs::write(outside.path().join("secret.go"), "secret\n").expect("write");
    symlink(
        outside.path().join("secret.go"),
        root.path().join("file_link.go"),
    )
    .expect("file symlink");
    symlink(outside.path(), root.path().join("dir_link")).expect("dir symlink");
    let source = Source::new(Some(root.path().to_path_buf()), None, String::new());

    assert_eq!(source.lines("file_link.go", 1, 1), None);
    assert_eq!(source.lines("dir_link/secret.go", 1, 1), None);
}
