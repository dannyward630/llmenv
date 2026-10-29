//! Derives the kebab-case slug that names a task file.

/// Derive a kebab-case slug from a task title: lowercase, first ~6 words,
/// non-alphanumeric runs collapsed to a single `-`, leading/trailing `-`
/// trimmed. Pure function — collision uniquification happens separately in
/// [`unique_slug`], which needs the store directory.
pub(crate) fn slugify(title: &str) -> String {
    let words: Vec<&str> = title.split_whitespace().take(6).collect();
    words
        .join(" ")
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::slugify;
    use proptest::prelude::*;

    proptest! {
        /// A slug names a file, so it holds only `[a-z0-9-]`, with no empty part between dashes.
        /// It keeps every ASCII letter and digit of the first six words, in order.
        #[test]
        fn slugify_is_a_file_safe_form_of_the_first_six_words(title in ".{0,80}") {
            let slug = slugify(&title);
            prop_assert!(
                slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{slug}"
            );
            prop_assert!(slug.split('-').all(|part| !part.is_empty()) || slug.is_empty(), "{slug}");

            let words: Vec<&str> = title.split_whitespace().take(6).collect();
            let kept: String = words
                .concat()
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .map(|c| c.to_ascii_lowercase())
                .collect();
            prop_assert_eq!(slug.replace('-', ""), kept);
            prop_assert_eq!(slugify(&slug), slug.clone());
        }
    }
}
