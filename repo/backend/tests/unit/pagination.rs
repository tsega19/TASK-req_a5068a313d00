use fieldops_backend::pagination::{PageParams, Paginated};
use serde_json::json;

fn p(page: Option<u32>, per_page: Option<u32>) -> PageParams {
    // Struct is Deserialize-only — build via JSON so we don't need private ctor.
    serde_json::from_value(json!({ "page": page, "per_page": per_page })).unwrap()
}

#[test]
fn defaults_when_missing() {
    let params = p(None, None);
    assert_eq!(params.effective(), (1, 20));
    assert_eq!(params.offset_limit(), (0, 20));
}

#[test]
fn page_zero_normalizes_to_one() {
    let params = p(Some(0), Some(10));
    assert_eq!(params.effective(), (1, 10));
}

#[test]
fn per_page_clamped_to_200() {
    let params = p(Some(1), Some(99999));
    assert_eq!(params.effective().1, 200);
}

#[test]
fn offset_math_for_page_3_per_25() {
    let params = p(Some(3), Some(25));
    assert_eq!(params.offset_limit(), (50, 25));
}

#[test]
fn paginated_envelope_shape() {
    let params = p(Some(2), Some(5));
    let out = Paginated::new(vec!["a", "b"], params, 42);
    let v = serde_json::to_value(out).unwrap();
    assert_eq!(v["page"], 2);
    assert_eq!(v["per_page"], 5);
    assert_eq!(v["total"], 42);
    assert_eq!(v["data"].as_array().unwrap().len(), 2);
}
