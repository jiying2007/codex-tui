//! An upstream queue item must never be silently downgraded to lossy plain text.
use codex_tui::{
    domain::ThreadId,
    thread_queue::{parse_queue_list, parse_submission},
};
use serde_json::{Value, json};
fn item(input: Value) -> Value {
    json!({"id":"item","clientUserMessageId":"client","input":input})
}
fn text() -> Value {
    json!({"type":"text","text":"visible", "textElements":[]})
}
fn no_loss(input: Value) {
    let parsed = parse_submission(&item(input.clone())).unwrap();
    assert_eq!(parsed.input, input.as_array().unwrap().clone());
    assert!(
        parsed.editable_text.is_none(),
        "structured or malformed content was declared plain-text editable"
    );
}
#[test]
fn text_elements_are_not_discarded_by_plain_text_editor() {
    let mut input = text();
    input["textElements"] = json!([{"kind":"reference","opaque":"retain"}]);
    no_loss(json!([input]));
}
#[test]
fn unknown_text_fields_remain_visible_but_not_lossily_editable() {
    let mut input = text();
    input["futureSemanticField"] = json!({"retain":true});
    no_loss(json!([input]));
}
#[test]
fn malformed_text_or_elements_are_not_editable() {
    for input in [
        json!({"type":"text"}),
        json!({"type":"text","text":17}),
        json!({"type":"text","text":"visible","textElements":null}),
        json!({"type":"text","text":"visible","textElements":{}}),
    ] {
        no_loss(json!([text(), input]));
    }
}
#[test]
fn empty_input_is_not_an_editable_plain_text_item() {
    no_loss(json!([]));
}
#[test]
fn duplicate_queue_ids_are_rejected() {
    let one = item(json!([text()]));
    assert!(
        parse_queue_list(
            ThreadId::new("t"),
            json!({"data":[one,one],"nextCursor":null})
        )
        .is_err()
    );
}
#[test]
fn empty_identifiers_are_rejected() {
    for key in ["id", "clientUserMessageId"] {
        for invalid in ["", " \n "] {
            let mut one = item(json!([text()]));
            one[key] = json!(invalid);
            assert!(parse_submission(&one).is_err());
        }
    }
}
#[test]
fn invalid_pagination_cursor_is_not_treated_as_complete() {
    for cursor in [json!(3), json!({}), json!([]), json!(""), json!(" ")] {
        assert!(
            parse_queue_list(
                ThreadId::new("t"),
                json!({"data":[item(json!([text()]))],"nextCursor":cursor})
            )
            .is_err()
        );
    }
}
#[test]
fn over_limit_queue_page_is_rejected() {
    let data: Vec<_> = (0..101)
        .map(|n| {
            let mut one = item(json!([text()]));
            one["id"] = json!(format!("q{n}"));
            one
        })
        .collect();
    assert!(parse_queue_list(ThreadId::new("t"), json!({"data":data,"nextCursor":null})).is_err());
}
#[test]
fn plain_text_old_optional_shape_and_unknown_envelope_fields_remain_supported() {
    let mut one = item(json!([{"type":"text","text":"first"},text()]));
    one["futureEnvelope"] = json!(true);
    let page = parse_queue_list(
        ThreadId::new("t"),
        json!({"data":[one],"nextCursor":null,"futureField":true}),
    )
    .unwrap();
    assert_eq!(
        page.submissions[0].editable_text.as_deref(),
        Some("first\nvisible")
    );
}
