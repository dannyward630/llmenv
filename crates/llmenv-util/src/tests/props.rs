use super::{dedup, merge_json};
use crate::testkit::arb_json;
use proptest::prelude::*;
use serde_json::Value;

proptest! {
    // dedup is idempotent, keeps first-seen order, and leaves no duplicates.
    #[test]
    fn dedup_is_idempotent_and_order_preserving(items in prop::collection::vec(0i32..6, 0..12)) {
        let mut once = items.clone();
        dedup(&mut once);
        let mut twice = once.clone();
        dedup(&mut twice);
        prop_assert_eq!(&once, &twice);
        let mut expected: Vec<i32> = Vec::new();
        for item in &items {
            if !expected.contains(item) {
                expected.push(*item);
            }
        }
        prop_assert_eq!(once, expected);
    }

    // normalize_json is idempotent.
    #[test]
    fn normalize_json_is_idempotent(v in arb_json()) {
        let mut once = v;
        super::super::normalize_json(&mut once);
        let mut twice = once.clone();
        super::super::normalize_json(&mut twice);
        prop_assert_eq!(once, twice);
    }

    // merge_json never panics on arbitrary input pairs.
    #[test]
    fn merge_json_total(mut dst in arb_json(), src in arb_json()) {
        merge_json(&mut dst, src);
    }

    // Disjoint object keys survive the merge; shared keys take src's value
    // when both are scalars (src wins on scalar collision).
    #[test]
    fn merge_json_src_scalar_wins_on_shared_key(
        key in "[a-z]{1,4}",
        a in any::<i32>(),
        b in any::<i32>(),
    ) {
        let mut dst = serde_json::json!({ &key: a });
        merge_json(&mut dst, serde_json::json!({ &key: b }));
        prop_assert_eq!(&dst[&key], &Value::from(b));
    }

    // Merging an object into itself is idempotent once arrays are
    // dedup-stable: re-merging the result changes nothing.
    #[test]
    fn merge_json_idempotent(v in arb_json()) {
        let mut once = v.clone();
        merge_json(&mut once, v.clone());
        let mut twice = once.clone();
        merge_json(&mut twice, once.clone());
        prop_assert_eq!(once, twice);
    }

    // Array merge output carries no duplicates (concat + dedup).
    #[test]
    fn merge_json_arrays_dedup(
        a in prop::collection::vec(0i32..5, 0..6),
        b in prop::collection::vec(0i32..5, 0..6),
    ) {
        let mut dst = Value::Array(a.iter().map(|n| Value::from(*n)).collect());
        merge_json(&mut dst, Value::Array(b.iter().map(|n| Value::from(*n)).collect()));
        let arr = dst.as_array().unwrap();
        let mut seen = arr.clone();
        dedup(&mut seen);
        prop_assert_eq!(arr.len(), seen.len(), "no duplicates in merged array");
    }

    // Stronger idempotence: merging ANY src into ANY dst is idempotent —
    // re-applying the same src to the merged result is a no-op. This holds
    // even when src's own arrays carry duplicates, because the merge
    // normalizes every array on insert as well as on recursive merge.
    #[test]
    fn merge_json_idempotent_for_arbitrary_pairs(
        dst in arb_json(),
        src in arb_json(),
    ) {
        let mut once = dst;
        merge_json(&mut once, src.clone());
        let mut twice = once.clone();
        merge_json(&mut twice, src);
        prop_assert_eq!(once, twice);
    }

    // Normalization is preserved: if `dst` is already dedup-free (the
    // real-world invariant — every `dst` is itself a prior merge_json
    // output), then merging arbitrary `src` keeps the output dedup-free at
    // every depth. The insert path normalizes src subtrees just like the
    // recursive-merge path, so output shape is independent of which path
    // produced a value.
    #[test]
    fn merge_json_preserves_normalization(
        dst in arb_json(),
        src in arb_json(),
    ) {
        // Establish the precondition by normalizing dst via a self-merge
        // into an empty object's key (merge_json is the normalizer).
        let mut normalized = Value::Null;
        merge_json(&mut normalized, dst);
        prop_assume!(all_arrays_deduped(&normalized));

        merge_json(&mut normalized, src);
        prop_assert!(
            all_arrays_deduped(&normalized),
            "merge introduced a non-deduped array: {normalized}"
        );
    }
}

// True iff every array nested anywhere in `v` contains no duplicates.
fn all_arrays_deduped(v: &Value) -> bool {
    match v {
        Value::Array(items) => {
            let mut seen = items.clone();
            dedup(&mut seen);
            seen.len() == items.len() && items.iter().all(all_arrays_deduped)
        }
        Value::Object(map) => map.values().all(all_arrays_deduped),
        _ => true,
    }
}
